extern crate termios;

use std::io::Write;
use std::io::{ErrorKind, Read};
use std::{fs::File, io, os::unix::io::FromRawFd};
use termios::{
    tcsetattr, Termios, BRKINT, CS8, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP, IXON, OPOST,
    TCSAFLUSH, VMIN, VTIME,
};

fn enter_raw_mode(tty_fd: i32, orig_termios: &Termios) {
    let mut termios = orig_termios.clone();

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

    tcsetattr(tty_fd, TCSAFLUSH, &termios).unwrap();
}

fn main() {
    let tty_fd = 0;
    let stdout = io::stdout();

    let orig_termios = Termios::from_fd(tty_fd).unwrap();
    enter_raw_mode(tty_fd, &orig_termios);

    let mut istream = unsafe { File::from_raw_fd(tty_fd) };
    loop {
        let mut buf = [0; 1];
        if let Err(e) = istream.read_exact(&mut buf) {
            if e.kind() == ErrorKind::UnexpectedEof {
                // just continue if the read call times out
                continue;
            }
        }

        let c = buf[0];
        if c.is_ascii_control() {
            print!("{}", c);
        } else {
            print!("{}", c as char);
        }

        stdout.lock().flush().unwrap();

        if c == b'q' {
            break;
        }
    }

    tcsetattr(tty_fd, TCSAFLUSH, &orig_termios).unwrap();
}
