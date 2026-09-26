use std::ops::Range;

use crate::buffer::Buffer;

pub fn join_lines(buf: &mut Buffer, line: usize) {
    let Some(l1) = buf.get_row(line) else {
        return;
    };
    let Some(l2) = buf.get_row(line + 1) else {
        return;
    };

    let l2 = l2.trim();

    let new_line = if l1.is_empty() {
        l2.to_owned()
    } else if l2.is_empty() {
        l1.to_owned()
    } else {
        let mut s = String::with_capacity(l1.len() + l2.len() + 1);
        s.push_str(l1);
        s.push(' ');
        s.push_str(l2);
        s
    };

    let l = l1.len();

    buf.start_action();
    buf.remove_lines(line..line + 2);
    buf.insert_lines(line, std::iter::once(new_line));
    buf.finish_action();
    buf.set_position(line, l);
}

#[derive(PartialEq, Eq, Debug)]
enum FindNumberState {
    Outside,
    Dash(usize),
    Inside(usize),
}

impl FindNumberState {
    fn advance(self, i: usize, ch: char) -> Self {
        use FindNumberState as S;
        match (self, ch) {
            (_, '-') => S::Dash(i),
            (S::Dash(i) | S::Inside(i), '0'..='9') => S::Inside(i),
            (S::Outside, '0'..='9') => S::Inside(i),
            (_, _) => S::Outside,
        }
    }

    fn until_inside(mut self, it: impl Iterator<Item = (usize, char)>) -> Self {
        if let Self::Inside(_) = self {
            return self;
        }
        for (i, ch) in it {
            self = self.advance(i, ch);
            if let Self::Inside(_) = self {
                return self;
            }
        }
        self
    }

    fn find_end(
        mut self,
        it: impl Iterator<Item = (usize, char)>,
        full_len: usize,
    ) -> Option<Range<usize>> {
        let start = match self {
            FindNumberState::Outside => return None,
            FindNumberState::Dash(i) | FindNumberState::Inside(i) => i,
        };
        for (i, ch) in it {
            self = self.advance(i, ch);
            match self {
                FindNumberState::Inside(_) => {}
                _ => return Some(start..i),
            }
        }
        Some(start..full_len)
    }
}

fn find_number(s: &str, start: usize) -> Option<Range<usize>> {
    let mut state = FindNumberState::Outside;

    let mut iter = s.char_indices();

    for (i, ch) in iter.by_ref().take(start + 1) {
        state = state.advance(i, ch);
    }

    state = state.until_inside(iter.by_ref());

    state.find_end(iter, s.len())
}

struct Number(bool, u64);

impl Number {
    fn parse(mut s: &str) -> Self {
        let neg = s.as_bytes()[0] == b'-';
        if neg {
            s = &s[1..];
        }
        let data = s.parse::<u64>().ok().unwrap_or(u64::MAX);
        Number(neg, data)
    }
    fn incr(self) -> Self {
        let Self(neg, num) = self;
        match (neg, num) {
            (false, u64::MAX) => Self(true, u64::MAX),
            (false, u) => Self(false, u + 1),
            (true, 1) => Self(false, 0),
            (true, 0) => Self(false, 1),
            (true, u) => Self(true, u - 1),
        }
    }

    fn decr(self) -> Self {
        let Self(neg, num) = self;
        match (neg, num) {
            (true, u64::MAX) => Self(false, u64::MAX),
            (true, u) => Self(true, u + 1),
            (false, 0) => Self(true, 1),
            (false, u) => Self(false, u - 1),
        }
    }

    fn space_for(&self) -> usize {
        let Self(neg, num) = *self;
        if num == 0 {
            return 1;
        }
        let ret = num.ilog10();
        if neg { ret as usize + 1 } else { ret as usize }
    }

    fn append_to(&self, f: &mut String) {
        use std::fmt::Write;
        if self.0 {
            f.push('-');
        }
        let _ = write!(f, "{}", self.1);
    }
}

fn modify_number_after_cursor(buf: &mut Buffer, f: impl FnOnce(Number) -> Number) {
    let Some(cur_line) = buf.get_row(buf.position().line()) else {
        return;
    };
    let Some(num) = find_number(cur_line, buf.position().col()) else {
        return;
    };

    let number = Number::parse(&cur_line[num.clone()]);
    let number = f(number);

    let mut line =
        String::with_capacity(num.start + number.space_for() + (cur_line.len() - num.end));
    line.push_str(&cur_line[..num.start]);
    number.append_to(&mut line);
    let col = line.chars().count().saturating_sub(1);
    line.push_str(&cur_line[num.end..]);

    let l = buf.position().line();
    buf.set_line(l, line);
    buf.set_position(l, col);
}

pub fn increment(buf: &mut Buffer) {
    modify_number_after_cursor(buf, Number::incr)
}

pub fn decrement(buf: &mut Buffer) {
    modify_number_after_cursor(buf, Number::decr)
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! find_number_tests {
        ($(
            $name:ident: $s:literal, $start:expr, $expected:expr;
        )*) => {
        $(
            #[test]
            fn $name() {
                assert_eq!(find_number($s, $start), $expected);
            }
        )*
        };
    }

    find_number_tests! {
        find_in_middle: "hello091world", 0, Some(5..8);
        find_at_end: "hello091", 0, Some(5..8);
        find_with_negative_end: "hello-091", 0, Some(5..9);
        find_with_negative_middle: "hello-091a", 0, Some(5..9);
        find_middle_of_number: "hello-091a", 7, Some(5..9);
        find_past_number: "hello-091a", 9, None;
        find_none: "hello world", 0, None;
        find_dashes: "hello-world", 0, None;
        find_double_dashes: "--10", 0, Some(1..4);
        find_many_dashes: "- - -10-", 0, Some(4..7);
    }
}
