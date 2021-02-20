extern crate termios;

use std::io::{ErrorKind, Read};
use std::{fs::File, io, os::unix::io::AsRawFd};
use termios::{
    tcsetattr, Termios, BRKINT, CS8, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP, IXON, OPOST,
    TCSAFLUSH, VMIN, VTIME,
};

fn raw_mode_params(termios: &mut Termios) {
    termios.c_cflag |= CS8; // set character size to 8 bits per byte (probably default)
    termios.c_iflag &= !(
        // don't translate break condition to SIGINT
        BRKINT
        // don't translate CR into NL
        | ICRNL
        // disable parity checking (obsolete?)
        | INPCK
        // disable stripping of 8th bit of each byte (probably off by default)
        | ISTRIP
        // disable Ctrl-S (stop transmittions to tty) and Ctrl-Q
        | IXON
    );
    termios.c_lflag &= !(
        // don't echo characters to terminal
        ECHO
        // disable canonical mode
        | ICANON
        // "implementation-defined" processing. Disables Ctrl-V?
        | IEXTEN
        // disable translation of Ctrl-C/Ctrl-Z to signals
        | ISIG
    );
    termios.c_oflag &= !OPOST; // don't translate \n to \r\n

    termios.c_cc[VMIN] = 0; // minimum number of charcters to read
    termios.c_cc[VTIME] = 1; // read waits 0.1 seconds
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

fn refresh_screen(output: &mut dyn std::io::Write) -> Result<(), std::io::Error> {
    output.write_all(b"\x1b[2J")?;
    output.flush()?;
    Ok(())
}

fn main() {
    let stdout = io::stdout();

    let mut input = File::open("/dev/tty").unwrap();
    let mut output = stdout.lock();

    let tty_fd = input.as_raw_fd();
    let orig_termios = Termios::from_fd(tty_fd).unwrap();
    let mut raw_termios = orig_termios.clone();
    raw_mode_params(&mut raw_termios);
    tcsetattr(tty_fd, TCSAFLUSH, &raw_termios).unwrap();

    loop {
        refresh_screen(&mut output).unwrap();
        if handle_input(&mut input).unwrap() {
            break;
        }
    }

    tcsetattr(tty_fd, TCSAFLUSH, &orig_termios).unwrap();
}
