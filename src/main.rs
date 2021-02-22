extern crate nix;

use nix::sys::termios;
use std::io::{ErrorKind, Read};
use std::{fs::File, os::unix::io::AsRawFd};

type GenericError = Box<dyn std::error::Error>;

nix::ioctl_read_bad!(
    ioctl_get_win_size,
    nix::libc::TIOCGWINSZ,
    nix::libc::winsize
);

#[derive(Debug, Clone, Copy)]
struct UVec2 {
    x: usize,
    y: usize,
}

fn get_window_size(fd: i32) -> Result<UVec2, GenericError> {
    let mut res = nix::libc::winsize {
        ws_col: 0,
        ws_row: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    unsafe {
        ioctl_get_win_size(fd, &mut res)?;
    }

    Ok(UVec2 {
        x: res.ws_col as usize,
        y: res.ws_row as usize,
    })
}

fn raw_mode_params(termios: &mut termios::Termios) {
    // set character size to 8 bits per byte (probably default)
    termios.control_flags |= termios::ControlFlags::CS8;

    termios.input_flags &= !(
        // don't translate break condition to SIGINT
        termios::InputFlags::BRKINT
        // don't translate CR into NL
        | termios::InputFlags::ICRNL
        // disable parity checking (obsolete?)
        | termios::InputFlags::INPCK
        // disable stripping of 8th bit of each byte (probably off by default)
        | termios::InputFlags::ISTRIP
        // disable Ctrl-S (stop transmittions to tty) and Ctrl-Q
        | termios::InputFlags::IXON
    );

    termios.local_flags &= !(
        // don't echo characters to terminal
        termios::LocalFlags::ECHO
        // disable canonical mode
        | termios::LocalFlags::ICANON
        // "implementation-defined" processing. Disables Ctrl-V?
        | termios::LocalFlags::IEXTEN
        // disable translation of Ctrl-C/Ctrl-Z to signals
        | termios::LocalFlags::ISIG
    );

    // don't translate \n to \r\n
    termios.output_flags &= !termios::OutputFlags::OPOST;

    // minimum number of charcters to read
    termios.control_chars[termios::SpecialCharacterIndices::VMIN as usize] = 0;

    // read waits 0.1 seconds
    termios.control_chars[termios::SpecialCharacterIndices::VTIME as usize] = 1;
}

fn ctrl_chord(c: u8) -> u8 {
    c & 0x1f
}

fn read_key(input: &mut dyn Read) -> Result<Option<u8>, std::io::Error> {
    let mut buf = [0; 1];
    if let Err(e) = input.read_exact(&mut buf) {
        // UnexpectedEof is generally a read timeout, which is safe to
        // ignore.
        if e.kind() == ErrorKind::UnexpectedEof {
            return Ok(None);
        }

        return Err(e);
    }
    return Ok(Some(buf[0]));
}

const SHOW_CURSOR: &'static [u8] = b"\x1b[?25h";
const HIDE_CURSOR: &'static [u8] = b"\x1b[?25l";
const CLEAR_SCREEN: &'static [u8] = b"\x1b[2J";
const CLEAR_LINE: &'static [u8] = b"\x1b[K";
const CURSOR_TO_START: &'static [u8] = b"\x1b[H";

struct Editor<'a> {
    curpos: UVec2,
    framebuf: Vec<u8>,
    quit: bool,

    tty_fd: i32,
    istream: &'a mut dyn std::io::Read,
    ostream: &'a mut dyn std::io::Write,

    term_settings: termios::Termios,
    prev_term_settings: termios::Termios,
    size: UVec2,
}

impl Editor<'_> {
    fn new<'a>(
        istream: &'a mut dyn std::io::Read,
        ostream: &'a mut dyn std::io::Write,
        tty_fd: i32,
    ) -> Result<Editor<'a>, GenericError> {
        let prev_term_settings = termios::tcgetattr(tty_fd)?;
        let mut term_settings = prev_term_settings.clone();
        raw_mode_params(&mut term_settings);

        Ok(Editor {
            curpos: UVec2 { x: 0, y: 0 },
            framebuf: Vec::new(),
            quit: false,
            istream,
            ostream,
            tty_fd,
            term_settings,
            prev_term_settings,
            size: get_window_size(tty_fd)?,
        })
    }

    fn apply_term_settings(&self) -> Result<(), GenericError> {
        termios::tcsetattr(self.tty_fd, termios::SetArg::TCSANOW, &self.term_settings)?;
        Ok(())
    }

    fn apply_prev_term_settings(&self) -> Result<(), GenericError> {
        termios::tcsetattr(
            self.tty_fd,
            termios::SetArg::TCSANOW,
            &self.prev_term_settings,
        )?;
        Ok(())
    }

    fn print(&mut self, content: &[u8]) {
        self.framebuf.append(&mut content.iter().cloned().collect());
    }

    fn flush(&mut self) -> Result<(), GenericError> {
        self.ostream.write_all(&self.framebuf)?;
        self.ostream.flush()?;
        self.framebuf.clear();
        Ok(())
    }

    fn handle_input(&mut self) -> Result<bool, GenericError> {
        let res = read_key(self.istream)?;
        if res.is_none() {
            return Ok(false);
        }

        let c = res.unwrap();

        // handle quit
        if c == ctrl_chord(b'q') {
            self.quit = true;
            return Ok(true);
        }

        // handle escape sequences
        if c == b'\x1b' {
            if let Some(true) = read_key(self.istream)?.map(|c| c == b'[') {
                match read_key(self.istream)? {
                    // up arrow
                    Some(b'A') => {
                        if self.curpos.y > 0 {
                            self.curpos.y -= 1;
                        }
                    }
                    // down arrow
                    Some(b'B') => {
                        if self.curpos.y < self.size.y - 1 {
                            self.curpos.y += 1;
                        }
                    }
                    // right arrow
                    Some(b'C') => {
                        if self.curpos.x < self.size.x - 1 {
                            self.curpos.x += 1;
                        }
                    }
                    // left arrow
                    Some(b'D') => {
                        if self.curpos.x > 0 {
                            self.curpos.x -= 1;
                        }
                    }
                    // do nothing by default
                    _ => (),
                }
            }
        }

        Ok(true)
    }

    fn update(&mut self) -> Result<bool, GenericError> {
        self.print(HIDE_CURSOR);
        self.print(CURSOR_TO_START);

        for i in 0..self.size.y {
            self.print(b"~");
            self.print(CLEAR_LINE);

            if i == self.size.y / 3 {
                let welcome = b"Welcome to textedit";
                let lmargin = (self.size.x - welcome.len()) / 2 - 1;
                for _ in 0..lmargin {
                    self.print(b" ");
                }
                self.print(welcome);
            }

            if i < self.size.y - 1 {
                self.print(b"\r\n");
            }
        }

        let move_cursor = format!("\x1b[{};{}H", self.curpos.y + 1, self.curpos.x + 1);
        self.print(move_cursor.as_bytes());
        self.print(SHOW_CURSOR);
        self.flush()?;

        while !self.handle_input()? {
            // wait for input before moving to next update
        }

        if self.quit {
            self.print(CLEAR_SCREEN);
            self.print(CURSOR_TO_START);
            self.flush()?;
            return Ok(true);
        }

        Ok(false)
    }
}

fn main() {
    let mut istream = File::open("/dev/tty").unwrap();
    let tty_fd = istream.as_raw_fd();
    let stdout = std::io::stdout();
    let mut ostream = stdout.lock();
    let mut e = Editor::new(&mut istream, &mut ostream, tty_fd).unwrap();

    e.apply_term_settings().unwrap();

    loop {
        match e.update() {
            Ok(true) => break,
            Ok(false) => (),
            Err(err) => {
                e.apply_prev_term_settings().unwrap();
                eprintln!("{:?}", err);
                std::process::exit(1);
            }
        }
    }

    e.apply_prev_term_settings().unwrap();
}
