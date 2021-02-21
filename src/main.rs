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
struct WindowSize {
    cols: usize,
    rows: usize,
}

fn get_window_size(fd: i32) -> Result<WindowSize, GenericError> {
    let mut res = nix::libc::winsize {
        ws_col: 0,
        ws_row: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    unsafe {
        ioctl_get_win_size(fd, &mut res)?;
    }

    Ok(WindowSize {
        cols: res.ws_col as usize,
        rows: res.ws_row as usize,
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

fn read_key(input: &mut dyn Read) -> Result<u8, std::io::Error> {
    let mut buf = [0; 1];
    loop {
        match input.read_exact(&mut buf) {
            Ok(()) => return Ok(buf[0]),
            Err(e) => {
                // UnexpectedEof is generally a read timeout, which is safe to
                // ignore. We should die on other errors.
                if e.kind() != ErrorKind::UnexpectedEof {
                    return Err(e);
                }
            }
        }
    }
}

const SHOW_CURSOR: &'static [u8] = b"\x1b[?25h";
const HIDE_CURSOR: &'static [u8] = b"\x1b[?25l";
const CLEAR: &'static [u8] = b"\x1b[2J";
const CURSOR_TO_START: &'static [u8] = b"\x1b[H";

struct Editor<'a> {
    framebuf: Vec<u8>,

    tty_fd: i32,
    istream: &'a mut dyn std::io::Read,
    ostream: &'a mut dyn std::io::Write,

    term_settings: termios::Termios,
    prev_term_settings: termios::Termios,
    size: WindowSize,
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
            framebuf: Vec::new(),
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
        Ok(())
    }

    fn render(&mut self) -> Result<(), GenericError> {
        self.print(HIDE_CURSOR);
        self.print(CLEAR);

        self.framebuf.clear();
        for i in 0..self.size.rows {
            self.framebuf.push(b'~');
            if i < self.size.rows - 1 {
                self.framebuf.push(b'\r');
                self.framebuf.push(b'\n');
            }
        }

        self.print(SHOW_CURSOR);
        self.flush()?;

        Ok(())
    }

    fn handle_input(&mut self) -> Result<bool, GenericError> {
        match read_key(self.istream)? {
            c if c == ctrl_chord(b'q') => Ok(true),
            _ => Ok(false),
        }
    }

    fn update(&mut self) -> Result<bool, GenericError> {
        self.render()?;

        if self.handle_input()? {
            self.print(CLEAR);
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
