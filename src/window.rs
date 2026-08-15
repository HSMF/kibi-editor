use crate::{
    CursorDirection,
    buffer::{Buffer, get_byte_range_from_char_range},
    location::Location,
};

const EMPTY_LINE: &str = if cfg!(test) { "~" } else { "\x1b[30m~\x1b[0m" };

pub struct Window {
    row_offset: usize,
    col_offset: usize,

    /// cursor as it was in the buffer
    prev_cursor: Location,

    /// cursor as it is on the screen
    cursor: Location,

    height: usize,
    width: usize,
}

impl Window {
    pub const fn new(width: usize, height: usize) -> Self {
        Self {
            row_offset: 0,
            col_offset: 0,
            prev_cursor: Location::new(0, 0),
            cursor: Location::new(0, 0),
            height,
            width,
        }
    }

    pub fn cursor(&self) -> Location {
        self.cursor
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// returns (virt_cursor, offset)
    fn scroll(
        pos: usize,
        old_pos: usize,
        max: usize,
        virt_cursor: usize,
        offset: usize,
    ) -> (usize, usize) {
        match pos.cmp(&old_pos) {
            // moved up
            std::cmp::Ordering::Less => {
                let diff = old_pos - pos;

                if virt_cursor > diff {
                    (virt_cursor - diff, offset)
                } else {
                    let diff = diff - virt_cursor;
                    (0, offset - diff)
                }
            }
            std::cmp::Ordering::Equal => (virt_cursor, offset),
            // moved down
            std::cmp::Ordering::Greater => {
                let diff = pos - old_pos;

                if virt_cursor + diff < max {
                    (virt_cursor + diff, offset)
                } else {
                    let diff = diff - (max - virt_cursor);
                    (max - 1, offset + diff + 1)
                }
            }
        }
    }

    pub fn follow_cursor(&mut self, buf: &Buffer) {
        let (line, col) = buf.position().destruct();
        let (old_line, old_col) = self.prev_cursor.destruct();
        let (mut cy, mut cx) = self.cursor.destruct();

        (cy, self.row_offset) = Self::scroll(line, old_line, self.height, cy, self.row_offset);
        (cx, self.col_offset) = Self::scroll(col, old_col, self.width, cx, self.col_offset);

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

    pub fn rows<'a>(&'a self, buf: &'a Buffer) -> impl IntoIterator<Item = &'a str> {
        // let (cx, cy) = self.fit_pos(buf);
        // self.cursor = Location::new(cy, cx);
        Rows {
            buf,
            win: self,
            y: 0,
        }
    }
}

pub struct Rows<'a> {
    buf: &'a Buffer,
    win: &'a Window,
    y: usize,
}

impl<'a> Iterator for Rows<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.y >= self.win.height {
            return None;
        }
        let ret = self
            .buf
            .get_row_render_full(self.win.row_offset + self.y)
            .map(|row| {
                let start = self.win.col_offset;
                let end = self.win.col_offset + self.win.width;
                &row[get_byte_range_from_char_range(row, start, end)]
            })
            .unwrap_or(EMPTY_LINE);
        self.y += 1;
        Some(ret)
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
        let got = win.rows(buf).into_iter().collect::<Vec<_>>();

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
}
