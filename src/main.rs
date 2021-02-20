extern crate termios;

use std::io::Write;
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

fn main() {
    let stdout = io::stdout();

    let mut istream = File::open("/dev/tty").unwrap();
    let tty_fd = istream.as_raw_fd();
    let orig_termios = Termios::from_fd(tty_fd).unwrap();

    let mut raw_termios = orig_termios.clone();
    raw_mode_params(&mut raw_termios);
    tcsetattr(tty_fd, TCSAFLUSH, &raw_termios).unwrap();

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

        if c == ctrl_chord(b'q') {
            break;
        }
    }

    tcsetattr(tty_fd, TCSAFLUSH, &orig_termios).unwrap();
}
