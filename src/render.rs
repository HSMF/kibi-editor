use tinyvec::{ArrayVec, array_vec};

pub const TAB_WIDTH: usize = 4;

pub struct RenderedChars<T> {
    inner: T,
    cur_len: usize,
}
pub trait IterExt: Sized {
    fn rendered(self) -> RenderedChars<Self>;
}

impl<I> IterExt for I
where
    I: Iterator<Item = char>,
{
    fn rendered(self) -> RenderedChars<Self> {
        RenderedChars {
            inner: self,
            cur_len: 0,
        }
    }
}

fn to_hex(x: u8) -> (char, char) {
    let lo = x & 0xf;
    let hi = x >> 4;
    let hex_dig = |ch| if ch < 10 { b'0' + ch } else { b'a' + (ch - 10) };
    (hex_dig(hi) as char, hex_dig(lo) as char)
}

impl<T> Iterator for RenderedChars<T>
where
    T: Iterator<Item = char>,
{
    type Item = ArrayVec<[char; 16]>;
    fn next(&mut self) -> Option<Self::Item> {
        let ch = self.inner.next()?;

        let ret = match ch {
            '\t' => {
                let spill = self.cur_len % TAB_WIDTH;
                let need = TAB_WIDTH - spill;
                let mut ret = ArrayVec::new();
                for _ in 0..need {
                    ret.push(' ');
                }
                ret
            }
            ch if ch.is_ascii_control() => {
                let (a, b) = to_hex(ch as u8);
                array_vec!(_ => 'X', a, b)
            }
            ch => array_vec!(_ => ch),
        };
        self.cur_len += ret.len();
        Some(ret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_cases {
        ($(
            $name:ident, $in:expr, $out:expr ;
        )*) => {
            $(
                #[test]
                fn $name() {
                    let s = $in;
                    let s = s.chars().rendered().flatten().collect::<String>();
                    assert_eq!(s, $out);
                }
            )*
        };
    }

    test_cases! {
        it_works, "hello world", "hello world";
        tab_simple, "\thello", "    hello";
        tab_nonfull, "a\thello", "a   hello";
        tab_middle, "abcd\thello", "abcd    hello";
        tab_small, "abc\thello", "abc hello";
    }
}
