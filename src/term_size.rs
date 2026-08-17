use std::{
    env::VarError,
    io::{Read, Write},
    mem::MaybeUninit,
    num::ParseIntError,
};

struct GetSizeError;

macro_rules! dummy_convert {
    ($($t:ty),* $(,)?) => {
        $(
            impl From<$t> for GetSizeError {
                fn from(_value: $t) -> Self {
                    GetSizeError
                }
            }
        )*
    };
}

dummy_convert!(VarError, ParseIntError, std::io::Error, std::str::Utf8Error);

fn from_ioctl() -> Result<(u16, u16), ()> {
    unsafe {
        let mut m: MaybeUninit<libc::winsize> = MaybeUninit::uninit();
        let r = libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, m.as_mut_ptr());
        if r == -1 {
            return Err(());
        }

        let winsize = m.assume_init();

        if winsize.ws_col == 0 {
            return Err(());
        }

        Ok((winsize.ws_col, winsize.ws_row))
    }
}

fn from_env() -> Result<(u16, u16), GetSizeError> {
    let lines = std::env::var("LINES")?.parse()?;
    let columns = std::env::var("COLUMNS")?.parse()?;

    Ok((lines, columns))
}

fn from_device_status_report() -> Result<(u16, u16), GetSizeError> {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(b"\x1b[999C\x1b[999B")?;
    stdout.write_all(b"\x1b[6n")?;
    stdout.flush()?;

    let mut stdin = std::io::stdin().lock();

    let mut buf = [0u8; 32];

    for i in 0..buf.len() - 1 {
        match stdin.read_exact(&mut buf[i..i + 1]) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        if buf[i] == b'R' {
            break;
        }
    }

    let (len, _) = buf
        .into_iter()
        .enumerate()
        .find(|(_, x)| *x == 0)
        .unwrap_or((0, b'0'));
    let buf = &buf[..len];
    if buf[0] != 0x1b || buf[1] != b'[' {
        return Err(GetSizeError);
    }

    let s = str::from_utf8(&buf[2..])?;
    let (rows, rest) = s.split_once(';').ok_or(GetSizeError)?;
    let cols = rest.strip_suffix('R').ok_or(GetSizeError)?;

    let rows = rows.parse()?;
    let cols = cols.parse()?;

    Ok((rows, cols))
}

pub fn get_terminal_size() -> Option<(u16, u16)> {
    if let Ok(x) = from_ioctl() {
        return Some(x);
    }

    if let Ok(x) = from_env() {
        return Some(x);
    }

    if let Ok(x) = from_device_status_report() {
        return Some(x);
    }

    None
}
