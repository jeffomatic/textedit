#[macro_use]
extern crate nix;

use nix::sys::termios;
use std::{
    error::Error,
    io::{ErrorKind, Read},
};
use std::{fs::File, io, os::unix::io::AsRawFd};

nix::ioctl_read_bad!(
    ioctl_get_win_size,
    nix::libc::TIOCGWINSZ,
    nix::libc::winsize
);

#[derive(Debug, Clone, Copy)]
struct WindowSize {
    rows: usize,
    cols: usize,
}

fn get_window_size(fd: i32) -> Result<WindowSize, Box<dyn Error>> {
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

fn handle_input(input: &mut dyn Read) -> Result<bool, std::io::Error> {
    match read_key(input)? {
        c if c == ctrl_chord(b'q') => Ok(true),
        _ => Ok(false),
    }
}

fn clear_screen(output: &mut dyn std::io::Write) -> Result<(), std::io::Error> {
    output.write_all(b"\x1b[2J")?; // 2J: erase in display, full screen
    output.write_all(b"\x1b[H")?; // H: position cursor, first col/row
    Ok(())
}

// VT100 escape sequence documentation:
// https://vt100.net/docs/vt100-ug/chapter3.html
fn refresh_screen(output: &mut dyn std::io::Write, rows: usize) -> Result<(), std::io::Error> {
    clear_screen(output)?;
    draw_rows(output, rows)?;
    output.flush()?;
    Ok(())
}

fn draw_rows(output: &mut dyn std::io::Write, rows: usize) -> Result<(), std::io::Error> {
    for _ in 0..rows {
        output.write_all(b"~\r\n")?;
    }

    Ok(())
}

fn main() {
    let stdout = io::stdout();

    let mut input = File::open("/dev/tty").unwrap();
    let mut output = stdout.lock();

    let tty_fd = input.as_raw_fd();

    let orig_termios = termios::tcgetattr(tty_fd).unwrap();
    let mut raw_termios = orig_termios.clone();
    raw_mode_params(&mut raw_termios);
    termios::tcsetattr(tty_fd, termios::SetArg::TCSAFLUSH, &raw_termios).unwrap();

    let size = get_window_size(tty_fd).unwrap();

    loop {
        refresh_screen(&mut output, size.rows).unwrap();
        if handle_input(&mut input).unwrap() {
            break;
        }
    }

    clear_screen(&mut output).unwrap();
    termios::tcsetattr(tty_fd, termios::SetArg::TCSAFLUSH, &orig_termios).unwrap();
}
