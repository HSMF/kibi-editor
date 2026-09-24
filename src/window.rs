use std::{fmt::Display, ops::Range};

use crate::{
    CursorDirection,
    buffer::{Buffer, char_idx_to_byte_idx, get_byte_range_from_char_range},
    location::Location,
    render::IterExt,
};

const EMPTY_LINE: &str = if cfg!(test) { "~" } else { "\x1b[30m~\x1b[0m" };

pub struct WindowOptions {
    pub number: bool,
}

impl WindowOptions {
    const fn new() -> Self {
        Self { number: false }
    }
}

pub struct Window {
    pub options: WindowOptions,
    row_offset: usize,
    col_offset: usize,

    /// cursor as it was in the buffer
    prev_cursor: Location,

    /// cursor as it is on the screen
    cursor: Location,

    height: usize,
    width: usize,

    pub(crate) visual: Option<Range<Location>>,
}

impl Window {
    pub const fn new(width: usize, height: usize) -> Self {
        Self {
            options: WindowOptions::new(),
            row_offset: 0,
            col_offset: 0,
            prev_cursor: Location::new(0, 0),
            cursor: Location::new(0, 0),
            height,
            width,
            visual: None,
        }
    }

    pub fn cursor(&self) -> Location {
        if self.options.number {
            self.cursor + (0, 4)
        } else {
            self.cursor
        }
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// returns (change in virt_cursor, change in offset) in terms of logical chars
    fn scroll(
        pos: usize,
        old_pos: usize,
        max: usize,
        virt_cursor: usize,
        get_diff: impl FnOnce(usize, usize) -> usize,
    ) -> (isize, isize) {
        match pos.cmp(&old_pos) {
            // moved up
            std::cmp::Ordering::Less => {
                let diff = get_diff(pos, old_pos);
                if virt_cursor > diff {
                    (-diff.cast_signed(), 0)
                } else {
                    let diff = diff - virt_cursor;
                    (-virt_cursor.cast_signed(), -diff.cast_signed())
                }
            }
            std::cmp::Ordering::Equal => (0, 0),
            // moved down
            std::cmp::Ordering::Greater => {
                let diff = get_diff(old_pos, pos);

                if (virt_cursor + diff) < max {
                    (diff.cast_signed(), 0)
                } else {
                    assert!(virt_cursor < max);
                    let available_space = max - virt_cursor - 1;
                    let diff_offset = diff - available_space;
                    assert!(diff > 0);
                    (available_space.cast_signed(), diff_offset.cast_signed())
                }
            }
        }
    }

    fn scroll_horizontal(&self, buf: &Buffer) -> (usize, usize) {
        let (line, col) = buf.position().destruct();
        let cur_row = buf.get_row(line).unwrap_or_default();

        let render_pos: usize = cur_row.chars().rendered().take(col).map(|x| x.len()).sum();
        let window_start = self.col_offset;
        let window_end = self.col_offset + self.width;
        let window = window_start..window_end;

        if window.contains(&render_pos) {
            return (render_pos - window_start, self.col_offset);
        }

        if render_pos >= window_end {
            // TODO: ensure that last_valid is start of a char
            let sub = 1;
            let last_valid = window_end - sub;
            let overshoot = render_pos - last_valid;
            return (self.width - sub, self.col_offset + overshoot);
        }

        // TODO: ensure that first_valid is start of a char
        let off = 0;
        let first_valid = window_start + off;
        let overshoot = first_valid - render_pos;
        (off, self.col_offset - overshoot)
    }

    pub fn follow_cursor(&mut self, buf: &Buffer) {
        let (line, col) = buf.position().destruct();
        let (old_line, _) = self.prev_cursor.destruct();
        let (mut cy, _) = self.cursor.destruct();

        let (d_cy, d_row_offset) =
            Self::scroll(line, old_line, self.height, cy, |start, end| end - start);
        cy = cy.wrapping_add_signed(d_cy);
        self.row_offset = self.row_offset.wrapping_add_signed(d_row_offset);
        let cx;
        (cx, self.col_offset) = self.scroll_horizontal(buf);

        self.cursor = Location::new(cy, cx);
        self.prev_cursor = Location::new(line, col);
    }

    pub fn move_window(&mut self, dir: CursorDirection) -> bool {
        let (mut line, col) = self.cursor.destruct();
        let mut moved = true;
        match dir {
            CursorDirection::Up if line < self.height - 1 && self.row_offset > 0 => {
                line += 1;
                self.row_offset -= 1;
            }
            CursorDirection::Down if line > 0 => {
                line -= 1;
                self.row_offset += 1;
            }
            CursorDirection::Left => todo!(),
            CursorDirection::Right => todo!(),
            _ => moved = false,
        }
        self.cursor = Location::new(line, col);
        moved
    }

    pub fn rows<'a>(&'a self, buf: &'a Buffer) -> impl IntoIterator<Item = Row<'a>> {
        // let (cx, cy) = self.fit_pos(buf);
        // self.cursor = Location::new(cy, cx);
        Rows {
            buf,
            win: self,
            y: 0,
        }
    }

    fn hl_for_row(&self, lnum: usize, row: &str) -> Range<usize> {
        const EMPTY: Range<usize> = 0..0;
        let Some(visual) = &self.visual else {
            return EMPTY;
        };

        if lnum < visual.start.line() || lnum > visual.end.line() {
            return EMPTY;
        }

        if lnum == visual.start.line() && lnum == visual.end.line() {
            return get_byte_range_from_char_range(row, visual.start.col(), visual.end.col());
        }

        if lnum == visual.start.line() {
            return char_idx_to_byte_idx(row, visual.start.col()).unwrap_or(0)..row.len();
        }

        if lnum == visual.end.line() {
            return 0..char_idx_to_byte_idx(row, visual.end.col()).unwrap_or(row.len());
        }

        0..row.len()
    }
}

fn project_onto(full: Range<usize>, window: &Range<usize>) -> Range<usize> {
    let projection = std::cmp::max(window.start, full.start)..std::cmp::min(window.end, full.end);

    projection.start.saturating_sub(window.start)..projection.end.saturating_sub(window.start)
}

pub struct Rows<'a> {
    buf: &'a Buffer,
    win: &'a Window,
    y: usize,
}

pub struct Row<'a> {
    row: &'a str,
    num: Option<usize>,
    hl: Range<usize>,
}

impl Display for Row<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(num) = self.num {
            // hack cos only 3 columns for numbers
            let num = num % 1000;
            write!(f, "\x1b[30m{num:>3}\x1b[0m ")?;
        }

        if self.hl.is_empty() {
            write!(f, "{}", self.row)?;
        } else {
            write!(f, "{}", &self.row[..self.hl.start])?;
            write!(
                f,
                "\x1b[40m{}\x1b[0m",
                &self.row[self.hl.start..self.hl.end]
            )?;
            write!(f, "{}", &self.row[self.hl.end..])?;
        }

        Ok(())
    }
}

impl<'a> Iterator for Rows<'a> {
    type Item = Row<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.y >= self.win.height {
            return None;
        }
        let start = self.win.col_offset;
        let mut end = self.win.col_offset + self.win.width;
        if self.win.options.number {
            end -= 4;
        }
        let row_render = self.buf.get_row_render_full(self.win.row_offset + self.y);
        let (ret, hl) = match row_render {
            Some(row) => {
                let hl = self.win.hl_for_row(self.win.row_offset + self.y, row);
                let range = get_byte_range_from_char_range(row, start, end);
                let hl = project_onto(hl, &range);
                let ret = &row[range];
                (Some(ret), hl)
            }
            None => (None, 0..0),
        };
        self.y += 1;
        if let Some(ret) = ret {
            Some(Row {
                row: ret,
                num: self
                    .win
                    .options
                    .number
                    .then_some(self.win.row_offset + self.y),
                hl,
            })
        } else {
            Some(Row {
                row: EMPTY_LINE,
                num: None,
                hl: 0..0,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_buf(s: &str) -> Buffer {
        let name = "t".to_owned();
        Buffer::read(name, textwrap::dedent(s).trim())
    }

    macro_rules! expected {
        [$($t:tt)*] => {
            expected2![ @INNER $($t)* ].collect::<Vec<_>>()
        };
    }

    macro_rules! expected2 {
        [@INNER ] => {
            std::iter::empty()
        };
        [@INNER ..$e:expr $(,)?] => {
            $e.into_iter()
        };
        [@INNER $e:expr $(,)?] => {
            std::iter::once($e)
        };
        [@INNER ..$e:expr, $($t:tt)* ] => {
            $e.into_iter().chain(expected2![@INNER $($t)*])
        };
        [@INNER $e:expr, $($t:tt)* ] => {
            std::iter::once($e).chain(expected2![@INNER $($t)*])
        };
    }

    fn draw_win(win: &Window, buf: &Buffer) {
        println!("+{}+", "-".repeat(win.width));
        let c = win.cursor();
        for (i, row) in win.rows(buf).into_iter().enumerate() {
            let row = row.to_string();
            print!("|");
            if i == c.line() {
                let before: String = row.chars().take(c.col()).collect();
                let ch = row.chars().nth(c.col()).unwrap_or(' ');
                let after: String = row.chars().skip(c.col() + 1).collect();
                print!("{before}\x1b[47;30m{ch}\x1b[0m{after}");
                // dbg!(before, ch, after, c, row);
            } else {
                print!("{row}");
            }
            let pad = win.width - row.chars().count();
            println!("{}|", " ".repeat(pad));
        }
        println!("+{}+", "-".repeat(win.width));
    }

    fn check_rows<'a>(
        win: &'a mut Window,
        buf: &'a Buffer,
        expected: impl IntoIterator<Item = &'a str>,
    ) {
        win.follow_cursor(buf);
        let expected = expected.into_iter().collect::<Vec<_>>();
        let height = win.height;
        let got = win
            .rows(buf)
            .into_iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>();
        let (line, col) = win.cursor().destruct();
        assert!(line < win.height(), "{line} >= {}", win.height());
        assert!(col < win.width, "{col} >= {}", win.width);

        draw_win(win, buf);
        assert_eq!(got, expected);
        assert_eq!(got.len(), height);
    }

    #[test]
    fn it_works() {
        let buf = new_buf(
            "
            hello
            world
            ",
        );
        let mut win = Window::new(50, 24);
        check_rows(
            &mut win,
            &buf,
            expected!["hello", "world", ..["~"].repeat(22)],
        );
    }

    #[test]
    fn scroll() {
        let mut buf = new_buf(&(0..=24).map(|x| x.to_string() + "\n").collect::<String>());
        let mut win = Window::new(50, 10);

        for i in 1..=9 {
            buf.move_cursor(crate::CursorDirection::Down);
            check_rows(
                &mut win,
                &buf,
                expected!["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"],
            );
            assert_eq!(win.cursor(), Location::new(i, 0));
        }

        buf.move_cursor(crate::CursorDirection::Down);
        check_rows(
            &mut win,
            &buf,
            expected!["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"],
        );

        for i in (0..9).rev() {
            buf.move_cursor(crate::CursorDirection::Up);
            check_rows(
                &mut win,
                &buf,
                expected!["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"],
            );
            assert_eq!(win.cursor(), Location::new(i, 0));
        }

        buf.move_cursor(crate::CursorDirection::Up);
        check_rows(
            &mut win,
            &buf,
            expected!["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"],
        );
        assert_eq!(win.cursor(), Location::new(0, 0));
    }

    #[test]
    fn jump() {
        let mut buf = new_buf(&(0..=24).map(|x| x.to_string() + "\n").collect::<String>());
        let mut win = Window::new(50, 10);

        buf.set_position(20, 0);
        check_rows(
            &mut win,
            &buf,
            expected![..(11..=20).map(|x| &*x.to_string().leak())],
        );

        buf.set_position(23, 0);
        check_rows(
            &mut win,
            &buf,
            expected![..(14..=23).map(|x| &*x.to_string().leak())],
        );

        buf.set_position(5, 0);
        check_rows(
            &mut win,
            &buf,
            expected![..(5..=14).map(|x| &*x.to_string().leak())],
        );
    }

    #[test]
    fn scroll_horizontal() {
        let mut buf = new_buf(
            &(0..=100)
                .map(|x| &*x.to_string().leak())
                .collect::<String>(),
        );
        let mut win = Window::new(10, 10);

        check_rows(&mut win, &buf, expected!["0123456789", ..["~"].repeat(9)]);
        buf.set_position(0, 8);
        check_rows(&mut win, &buf, expected!["0123456789", ..["~"].repeat(9)]);
        buf.move_cursor(crate::CursorDirection::Right);
        check_rows(&mut win, &buf, expected!["0123456789", ..["~"].repeat(9)]);
        buf.move_cursor(crate::CursorDirection::Right);
        check_rows(&mut win, &buf, expected!["1234567891", ..["~"].repeat(9)]);
        buf.move_cursor(crate::CursorDirection::Right);
        check_rows(&mut win, &buf, expected!["2345678910", ..["~"].repeat(9)]);
        buf.move_cursor(crate::CursorDirection::Right);
        check_rows(&mut win, &buf, expected!["3456789101", ..["~"].repeat(9)]);
        buf.move_cursor(crate::CursorDirection::Right);
        check_rows(&mut win, &buf, expected!["4567891011", ..["~"].repeat(9)]);

        buf.set_position(0, 2);
        check_rows(&mut win, &buf, expected!["2345678910", ..["~"].repeat(9)]);
        buf.set_position(0, 1);
        check_rows(&mut win, &buf, expected!["1234567891", ..["~"].repeat(9)]);
    }

    #[test]
    fn tabs() {
        let name = "t".to_owned();
        let mut buf = Buffer::read(name, "\thello\tworld");
        let mut win = Window::new(10, 10);
        check_rows(&mut win, &buf, expected!["    hello ", ..["~"].repeat(9)]);
        buf.move_cursor(CursorDirection::Right);
        check_rows(&mut win, &buf, expected!["    hello ", ..["~"].repeat(9)]);
        assert_eq!(win.cursor(), Location::new(0, 4));
    }

    #[test]
    fn delete() {
        let name = "t".to_owned();
        let mut buf = Buffer::read(name, "hello");
        buf.set_position(0, 5);
        let mut win = Window::new(10, 10);
        check_rows(&mut win, &buf, expected!["hello", ..["~"].repeat(9)]);

        assert_eq!(buf.position(), win.cursor());

        buf.delete_range(Location::new(0, 3), Location::new(0, 5));
        check_rows(&mut win, &buf, expected!["hel", ..["~"].repeat(9)]);
        assert_eq!(buf.position(), win.cursor());
        assert_eq!(buf.position(), Location::new(0, 2));
        assert_eq!(win.cursor(), Location::new(0, 2));
    }

    #[test]
    fn delete_to_scroll() {
        let name = "t".to_owned();
        let mut buf = Buffer::read(name, "hello world foo bar baz");
        buf.set_position(0, 22);
        let mut win = Window::new(10, 10);
        check_rows(&mut win, &buf, expected!["oo bar baz", ..["~"].repeat(9)]);

        buf.delete_range(Location::new(0, 11), Location::new(0, 23));
        check_rows(&mut win, &buf, expected!["d", ..["~"].repeat(9)]);
    }

    #[test]
    fn delete_in_insert() {
        let name = "t".to_owned();
        let mut buf = Buffer::read(name, "hello world foo bar baz");
        let mut win = Window::new(10, 10);
        buf.set_position(0, 22);
        check_rows(&mut win, &buf, expected!["oo bar baz", ..["~"].repeat(9)]);

        buf.set_go_past_end(true);
        buf.set_position(0, 23);
        check_rows(&mut win, &buf, expected!["o bar baz", ..["~"].repeat(9)]);

        buf.delete_range(buf.position() - (0, 1), buf.position());
        check_rows(&mut win, &buf, expected!["o bar ba", ..["~"].repeat(9)]);

        assert_eq!(win.cursor(), Location::new(0, 8));
    }

    #[test]
    fn delete_middle() {
        let name = "t".to_owned();
        let mut buf = Buffer::read(name, "hello world foo bar baz");
        let mut win = Window::new(10, 10);
        buf.set_position(0, 15);
        check_rows(&mut win, &buf, expected!["world foo ", ..["~"].repeat(9)]);

        buf.set_position(0, 13);
        buf.delete_range(buf.position() - (0, 1), buf.position());
        check_rows(&mut win, &buf, expected!["world oo b", ..["~"].repeat(9)]);
    }

    #[test]
    fn delete_tab() {
        let name = "t".to_owned();
        let mut buf = Buffer::read(name, "\tabcdef");
        let mut win = Window::new(10, 10);
        buf.set_position(0, 1);
        check_rows(&mut win, &buf, expected!["    abcdef", ..["~"].repeat(9)]);
        buf.delete_range(Location::new(0, 0), Location::new(0, 1));
        check_rows(&mut win, &buf, expected!["abcdef", ..["~"].repeat(9)]);
        assert_eq!(win.cursor(), Location::new(0, 0));
    }

    #[test]
    fn many_tabs() {
        let name = "t".to_owned();
        let mut buf = Buffer::read(name, "\ta\tb\tcd\te");
        // render: "    a   b   cd  e"
        let mut win = Window::new(10, 10);
        buf.set_position(0, 1);
        check_rows(&mut win, &buf, expected!["    a   b ", ..["~"].repeat(9)]);
        buf.set_position(0, 6);
        check_rows(&mut win, &buf, expected!["a   b   cd", ..["~"].repeat(9)]);
        buf.set_position(0, 5);
        assert_eq!(buf.position(), Location::new(0, 5));
        check_rows(&mut win, &buf, expected!["a   b   cd", ..["~"].repeat(9)]);
    }
}
