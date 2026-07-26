use std::{
    collections::VecDeque,
    ops::{Range, RangeInclusive},
};

use tinyvec::{ArrayVec, array_vec};

const UNDOLEVEL: usize = 1000;

use crate::{CursorDirection, location::Location};

#[derive(PartialEq, Eq, Debug)]
enum Action {
    RemoveLine {
        line_num: usize,
        line: String,
    },
    RemoveLines {
        range: Range<usize>,
        lines: Vec<String>,
    },
    InsertChar {
        inserted: char,
        at: Location,
    },
    AddNewline {
        at: Location,
    },
    DeleteRange {
        start: Location,
        end: Location,
        /// what was removed
        range: String,
    },
    SetRange {
        start: Location,
        end: Location,
        /// what was there previously
        replaced: Vec<String>,
    },
    InsertLines {
        start: usize,
        count: usize,
    },
    /// group of actions that make up just one action on the stack
    Group(Vec<Action>),
}

#[derive(Debug, PartialEq, Eq, Default)]
pub struct Row {
    content: String,
    render: String,
    /// number of *visible* chars in `render`
    render_chars: usize,
}

fn to_hex(x: u8) -> (char, char) {
    let lo = x & 0xf;
    let hi = x >> 4;
    let hex_dig = |ch| if ch < 10 { b'0' + ch } else { b'a' + (ch - 10) };
    (hex_dig(hi) as char, hex_dig(lo) as char)
}

impl Row {
    fn rendered_char(ch: char) -> ArrayVec<[char; 16]> {
        match ch {
            '\t' => array_vec!(_ => ' ', ' ', ' ', ' '),
            ch if ch.is_ascii_control() => {
                let (a, b) = to_hex(ch as u8);
                array_vec!(_ => 'X', a, b)
            }
            ch => array_vec!(_ => ch),
        }
    }

    fn rendered(s: &str) -> (String, usize) {
        let mut ret = String::new();
        let mut len = 0;
        for ch in s.chars() {
            for r in Self::rendered_char(ch) {
                ret.push(r)
            }
            len += 1;
        }
        (ret, len)
    }
    pub fn new(content: String) -> Self {
        let (render, len) = Self::rendered(&content);
        Self {
            content,
            render_chars: len,
            render,
        }
    }

    pub fn render_len(&self) -> usize {
        self.render.chars().count()
    }

    pub fn content_len(&self) -> usize {
        self.content.chars().count()
    }

    fn cx_to_rendered(&self, cx: u16) -> u16 {
        self.content
            .chars()
            .take(cx.into())
            .map(|ch| Self::rendered_char(ch).len() as u16)
            .sum()
    }

    fn insert_char(&mut self, ch: char, cur_col: usize) {
        if cur_col == self.content_len() {
            self.content.push(ch);
            for ch in Self::rendered_char(ch) {
                self.render.push(ch);
            }
        } else {
            let idx = char_idx_to_byte_idx(&self.content, cur_col).expect("byte index exists");
            self.content.insert(idx, ch);
            self.recompute_rendered();
        }
    }

    fn recompute_rendered(&mut self) {
        (self.render, self.render_chars) = Self::rendered(&self.content);
    }

    fn split(&mut self, cur_col: usize) -> Row {
        let (i, _) = self
            .content
            .char_indices()
            .nth(cur_col)
            .unwrap_or((self.content.len(), ' '));
        let (before, after) = self.content.split_at(i);
        let after = after.to_owned();
        self.content.truncate(before.len());
        self.recompute_rendered();
        Row::new(after)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Buffer {
    row: Vec<Row>,
    row_off: usize,
    col_off: usize,
    name: String,
    path: Option<String>,
    cur_line: usize,
    cur_col: usize,
    dirty: bool,

    allow_one_past: bool,

    actions: VecDeque<Action>,
    redo_actions: Vec<Action>,

    in_cur_group: bool,
    cur_group: Vec<Action>,
}

pub(crate) fn get_byte_range_from_char_range(s: &str, start: usize, end: usize) -> Range<usize> {
    let mut sb = None;
    let mut eb = s.len();
    for (i, (byte, _)) in s.char_indices().enumerate() {
        if i == start {
            sb = Some(byte)
        }
        if i == end {
            eb = byte
        }
    }
    if let Some(sb) = sb { sb..eb } else { 0..0 }
}

pub(crate) fn char_idx_to_byte_idx(s: &str, idx: usize) -> Option<usize> {
    s.char_indices().nth(idx).map(|x| x.0)
}

/// applies `f` to every element of `i` but the last, which it returns
fn iter_split_last<T>(mut i: impl Iterator<Item = T>, mut f: impl FnMut(T)) -> Option<T> {
    let mut cur = i.next()?;
    for item in i {
        f(cur);
        cur = item;
    }
    Some(cur)
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            row: vec![],
            row_off: 0,
            col_off: 0,
            cur_line: 0,
            cur_col: 0,
            name: String::new(),
            path: None,
            dirty: false,
            allow_one_past: false,
            actions: VecDeque::new(),
            redo_actions: Vec::new(),

            in_cur_group: false,
            cur_group: Vec::new(),
        }
    }

    pub fn read(name: String, s: &str) -> Self {
        let row = s.lines().map(|line| Row::new(line.to_owned())).collect();
        Self {
            row,
            row_off: 0,
            col_off: 0,
            cur_col: 0,
            cur_line: 0,
            path: Some(name.clone()),
            name,
            dirty: false,
            allow_one_past: false,
            actions: VecDeque::new(),
            redo_actions: Vec::new(),

            in_cur_group: false,
            cur_group: Vec::new(),
        }
    }

    /// sets whether it is allowed to be one past the end of the line
    pub fn set_go_past_end(&mut self, allow: bool) {
        self.allow_one_past = allow;
        self.clamp_cursor();
    }

    fn clamp_cursor(&mut self) {
        let limit = self.row_len(self.cur_line);
        let limit = if self.allow_one_past {
            limit
        } else {
            limit.saturating_sub(1)
        };
        self.cur_col = self.cur_col.clamp(0, limit);
        self.cur_line = self.cur_line.clamp(0, self.row.len().saturating_sub(1))
    }

    /// mark as "not dirty"
    pub fn scrub(&mut self) {
        self.dirty = false;
    }

    pub fn save(&self) -> String {
        let len = self.row.iter().map(|x| x.content.len() + 1).sum();
        let mut ret = String::with_capacity(len);

        for row in self.row.iter() {
            ret.push_str(&row.content);
            ret.push('\n');
        }

        ret
    }

    fn row_len(&self, row: usize) -> usize {
        self.row.get(row).map(Row::content_len).unwrap_or(0)
    }

    pub fn get_row(&self, row: usize) -> Option<&str> {
        self.row.get(row).map(|x| &*x.content)
    }

    pub fn get_row_render(&self, row: usize, width: usize) -> Option<&str> {
        self.row.get(self.row_off + row).map(|row| {
            let start = self.col_off;
            let end = self.col_off + width;
            &row.render[get_byte_range_from_char_range(&row.render, start, end)]
        })
    }

    pub fn do_remove_line(&mut self, line: usize) -> String {
        self.row.remove(line).content
    }

    pub fn is_empty(&self) -> bool {
        self.row.is_empty()
    }

    pub fn len(&self) -> usize {
        self.row.len()
    }

    /// cx, cy are the coordinates in the current window.
    /// requires cx+self.col_off >= 0 && cy+self.row_off >= 0
    ///
    /// rows, cols are the dimensions of the screen.
    ///
    /// returns the new virtual coordinates.
    ///
    /// ensures
    /// ret.0 < cols && ret.1 < rows
    pub fn fit_pos(&mut self, cx: i32, cy: i32, rows: u16, cols: u16) -> (u16, u16) {
        let row_len = self
            .row
            .get(self.row_off.checked_add_signed(cy as isize).unwrap_or(0))
            .map(Row::render_len)
            .unwrap_or(0);

        // if (0 <= cx && cx < cols.into() && (cx as usize) < row_len) && (0 <= cy && cy < rows.into())
        // {
        //     // no scroll needed
        //     return (cx as u16, cy as u16);
        // }

        let max_row_off = self.len().saturating_sub(rows as usize);

        let max_col_off = row_len.saturating_sub(cols as usize);

        let mut cy = cy;
        let mut cx = cx;
        if cy >= rows.into() {
            // need to scroll down
            let scroll_by = cy - rows as i32 + 1;
            self.row_off += scroll_by as usize;
            cy = rows as i32 - 1;
        }

        if cy < 0 {
            // need to scroll up
            let scroll_by = -cy;
            self.row_off = self.row_off.saturating_sub(scroll_by as usize);
            cy = 0;
        }

        if cx >= cols.into() {
            let scroll_by = cx - cols as i32 + 1;
            self.col_off += scroll_by as usize;
            cx = cols as i32 - 1;
        }

        if cx < 0 {
            let scroll_by = -cx;
            self.col_off = self.col_off.saturating_sub(scroll_by as usize);
            cx = 0;
        }

        self.row_off = self.row_off.clamp(0, max_row_off);
        self.col_off = self.col_off.clamp(0, max_col_off);
        cx = std::cmp::min(cx as usize, row_len.saturating_sub(1)) as i32;
        cy = std::cmp::min(cy as usize, self.len().saturating_sub(1)) as i32;

        (cx.try_into().unwrap(), cy.try_into().unwrap())
    }

    fn cx_to_rendered(&self, cx: u16) -> u16 {
        let Some(row) = self.row.get(self.cur_line) else {
            return 0;
        };

        row.cx_to_rendered(cx)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn scroll_to_fit(&mut self, rows: u16, cols: u16) {
        let cx = (self.cur_col as isize - self.col_off as isize) as i32;
        let cy = (self.cur_line as isize - self.row_off as isize) as i32;

        self.fit_pos(cx, cy, rows, cols);
    }

    pub fn move_cursor(&mut self, c: CursorDirection) {
        use CursorDirection as C;

        match c {
            C::Up => self.cur_line = self.cur_line.saturating_sub(1),
            C::Down => self.cur_line += 1,
            C::Left => self.cur_col = self.cur_col.saturating_sub(1),
            C::Right => self.cur_col += 1,
        }

        self.clamp_cursor();
    }

    /// where to place the cursor (rows x cols coordinates)
    /// returns (y, x)
    pub fn cursor(&mut self, rows: u16, cols: u16) -> (u16, u16) {
        self.scroll_to_fit(rows, cols);
        (
            (self.cur_line - self.row_off).try_into().unwrap(),
            self.cx_to_rendered((self.cur_col - self.col_off).try_into().unwrap()),
        )
    }

    /// returns (line, col)
    pub fn position(&self) -> Location {
        Location::new(self.cur_line, self.cur_col)
    }

    /// returns true if `pos` is a valid position in the buffer
    pub fn contains_position(&self, pos: Location) -> bool {
        let Some(line) = self.get_row(pos.line()) else {
            return false;
        };
        pos.col() == 0 || pos.col() < line.chars().count()
    }

    /// returns (line, col)
    pub fn set_position(&mut self, line: usize, col: usize) {
        self.cur_line = line;
        self.cur_col = col;
        self.clamp_cursor();
    }

    fn do_insert_char(&mut self, ch: char) {
        self.dirty = true;
        if self.row.is_empty() {
            self.row.push(Row::new(String::with_capacity(1)));
        }
        let row = &mut self.row[self.cur_line];

        row.insert_char(ch, self.cur_col);

        self.move_cursor(CursorDirection::Right);
    }

    fn do_add_newline(&mut self) {
        if self.row.is_empty() {
            self.row.push(Row::new(String::new()));
        }
        self.dirty = true;
        let row = &mut self.row[self.cur_line];
        let next = row.split(self.cur_col);
        self.row.insert(self.cur_line + 1, next);

        self.cur_line += 1;
        self.cur_col = 0;
    }

    pub(crate) fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub fn num_lines(&self) -> usize {
        self.row.len()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn set_path(&mut self, path: String) {
        self.path = Some(path);
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    /// behaves like `nvim_buf_get_text`
    /// lines inclusive, columns exclusive
    pub fn get_range<'a>(&'a self, start: Location, end: Location) -> RangeIter<'a> {
        RangeIter {
            buf: self,
            start,
            end,
        }
    }

    /// behaves like `nvim_buf_get_lines`
    pub fn get_lines<'a>(&'a self, range: RangeInclusive<usize>) -> RangeLinesIter<'a> {
        RangeLinesIter {
            buf: self,
            start: *range.start(),
            end: *range.end(),
        }
    }

    /// lines inclusive, columns exclusive
    // TODO: do we want to return the deleted text?
    fn do_delete_range(&mut self, start: Location, end: Location) -> String {
        self.dirty = true;
        assert!(start <= end);
        let mut ret = vec![];
        let mut last_line = None;
        let range = if start.line() == end.line() {
            let row = &mut self.row[start.line()];
            let drain = row.content.drain(get_byte_range_from_char_range(
                &row.content,
                start.col(),
                end.col(),
            ));
            ret.push(drain.collect::<String>());
            row.recompute_rendered();
            0..0
        } else {
            let mut end_row = self.row.remove(end.line()).content;
            let row = &mut self.row[start.line()];
            if let Some(start) = char_idx_to_byte_idx(&row.content, start.col()) {
                let drain = row.content.drain(start..);
                ret.push(drain.collect());
            } else {
                ret.push(String::new());
            }

            let drain = end_row
                .drain(0..char_idx_to_byte_idx(&end_row, end.col()).unwrap_or(end_row.len()));
            last_line = Some(drain.collect());
            row.content.push_str(&end_row);

            row.recompute_rendered();

            start.line() + 1..end.line()
        };
        let drain = self.row.drain(range);
        ret.extend(drain.map(|x| x.content));

        if start <= self.position() && self.position() <= end {
            self.set_position(start.line(), start.col());
        }
        if let Some(l) = last_line {
            ret.push(l);
        }
        ret.join("\n")
    }

    fn do_set_range<I, S>(&mut self, start: Location, end: Location, replacement: I) -> Action
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let (replaced, end) = self.do_set_range_(start, end, replacement);
        self.save();
        Action::SetRange {
            start,
            end,
            replaced,
        }
    }

    /// lines inclusive, columns exclusive
    ///
    /// returns the lines that were replaced and the new end of the range inserted
    fn do_set_range_<I, S>(
        &mut self,
        start: Location,
        end: Location,
        replacement: I,
    ) -> (Vec<String>, Location)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.dirty = true;
        let mut replacement = replacement.into_iter().map(Into::into);
        let mut ret = vec![];

        // what nasty logic
        let end_loc = if start.line() == end.line() {
            let row = &mut self.row[start.line()];
            let first = replacement.next().unwrap_or_default();
            let second = replacement.next();

            match second {
                None => {
                    // same line
                    let row = std::mem::take(row);
                    let mut content = row.content;
                    let drain = content.drain(get_byte_range_from_char_range(
                        &content,
                        start.col(),
                        end.col(),
                    ));
                    ret.push(drain.collect());
                    let idx = char_idx_to_byte_idx(&content, start.col()).unwrap_or(content.len());
                    let content = content.into_bytes();

                    let l = Location::new(start.line(), start.col() + first.chars().count());
                    let content = insert_in_middle(content, idx, first.bytes());
                    let content = String::from_utf8(content).expect("valid utf8");

                    self.row[start.line()] = Row::new(content);

                    l
                }
                Some(second) => {
                    // split into lines
                    let mut lines: Vec<_> = std::iter::once(second).chain(replacement).collect();
                    let mut last = lines.pop().expect("not empty");
                    let loc = Location::new(start.line() + lines.len() + 1, last.chars().count());

                    let start_idx = char_idx_to_byte_idx(&row.content, start.col())
                        .unwrap_or(row.content_len());
                    let end_idx =
                        char_idx_to_byte_idx(&row.content, end.col()).unwrap_or(row.content_len());

                    last.extend(row.content.drain(end_idx..));

                    ret.push(row.content.drain(start_idx..).collect());
                    row.content.push_str(&first);
                    row.recompute_rendered();

                    self.row = insert_in_middle(
                        std::mem::take(&mut self.row),
                        start.line() + 1,
                        lines.into_iter().chain(std::iter::once(last)).map(Row::new),
                    );

                    loc
                }
            }
        } else {
            let first = replacement.next().unwrap_or_default();
            let second = replacement.next();

            match second {
                None => {
                    let row = &mut self.row[start.line()];
                    let idx = char_idx_to_byte_idx(&row.content, start.col())
                        .unwrap_or(row.content.len());
                    ret.push(row.content.drain(idx..).collect());

                    let remove_lines = self.row.drain(start.line() + 1..=end.line());
                    let mut last = iter_split_last(remove_lines, |row| ret.push(row.content))
                        .expect("at least one row")
                        .content;

                    let row = &mut self.row[start.line()];
                    let idx = char_idx_to_byte_idx(&last, end.col()).unwrap_or(last.len());
                    row.content.push_str(&first);
                    ret.push(last.drain(..idx).collect());
                    row.content.push_str(&last);
                    row.recompute_rendered();

                    Location::new(start.line(), start.col() + first.chars().count())
                }
                Some(_) => todo!("set range {start:?} {end:?}"),
            }
        };

        if start <= self.position() && self.position() <= end {
            self.set_position(start.line(), start.col());
        }

        (ret, end_loc)
    }

    fn do_remove_lines(&mut self, range: Range<usize>) -> Action {
        self.dirty = true;
        let lines = self.row.drain(range.clone()).map(|x| x.content).collect();

        Action::RemoveLines { range, lines }
    }

    /// insert `lines` before `start`
    fn do_insert_lines<I, S>(&mut self, start: usize, lines: I) -> Action
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let before = self.row.len();
        let inner = || {
            let lines = lines.into_iter().map(Into::into).map(Row::new);
            if start > self.row.len() {
                self.row.extend(lines);
                return;
            }
            self.row = insert_in_middle(std::mem::take(&mut self.row), start, lines);
        };
        inner();
        let after = self.row.len();
        let count = after - before;
        Action::InsertLines { start, count }
    }

    fn push_action(&mut self, act: Action) {
        self.redo_actions.clear();
        if self.in_cur_group {
            self.cur_group.push(act);
            return;
        }
        if self.actions.len() == UNDOLEVEL {
            self.actions.pop_front();
        }
        self.actions.push_back(act);
    }

    /// starts a group of actions that are treated atomically
    pub fn start_action(&mut self) {
        assert!(!self.in_cur_group, "todo: handle nested groups");
        self.in_cur_group = true;
    }

    /// finishes a group of actions that are treated atomically
    pub fn finish_action(&mut self) {
        self.in_cur_group = false;
        if !self.cur_group.is_empty() {
            let group = std::mem::take(&mut self.cur_group);
            self.push_action(Action::Group(group));
        }
    }
}

// buffer mutating operations
impl Buffer {
    pub fn remove_line(&mut self, line: usize) -> String {
        let line_num = line;
        let line = self.do_remove_line(line_num);
        self.push_action(Action::RemoveLine {
            line_num,
            line: line.clone(),
        });
        line
    }

    pub fn insert_char(&mut self, ch: char) {
        self.push_action(Action::InsertChar {
            inserted: ch,
            at: self.position(),
        });
        self.do_insert_char(ch)
    }

    pub fn add_newline(&mut self) {
        self.push_action(Action::AddNewline {
            at: self.position(),
        });
        self.do_add_newline()
    }

    /// lines inclusive, columns exclusive
    // TODO: do we want to return the deleted text?
    pub fn delete_range(&mut self, start: Location, end: Location) {
        if self.row.is_empty() {
            return;
        }
        let range = self.do_delete_range(start, end);
        self.push_action(Action::DeleteRange { start, end, range });
    }

    /// lines inclusive, columns exclusive
    pub fn set_range<I, S>(&mut self, start: Location, end: Location, replacement: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let action = self.do_set_range(start, end, replacement);
        self.push_action(action);
    }

    pub fn remove_lines(&mut self, range: Range<usize>) {
        let action = self.do_remove_lines(range.clone());
        self.push_action(action);
    }

    /// insert `lines` before `start`
    pub fn insert_lines<I, S>(&mut self, start: usize, lines: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let action = self.do_insert_lines(start, lines);
        self.push_action(action);
    }

    fn do_undo(&mut self, action: Action) -> Action {
        match action {
            Action::RemoveLine { line_num, line } => {
                self.do_insert_lines(line_num, std::iter::once(line))
            }
            Action::RemoveLines { range, lines } => self.do_insert_lines(range.start, lines),
            Action::InsertChar { inserted, at } => {
                let line = &mut self.row[at.line()];
                let idx = char_idx_to_byte_idx(&line.content, at.col()).unwrap();
                assert_eq!(line.content.remove(idx), inserted);
                line.recompute_rendered();
                Action::DeleteRange {
                    start: at,
                    end: at + (0, 1),
                    range: [inserted].iter().collect(),
                }
            }
            Action::AddNewline { at } => {
                let next = self.row.remove(at.line() + 1);
                let line = &mut self.row[at.line()];
                line.content.push_str(&next.content);
                line.recompute_rendered();
                Action::DeleteRange {
                    start: at,
                    end: Location::new(at.line() + 1, 0),
                    range: ['\n'].iter().collect(),
                }
            }
            Action::DeleteRange {
                start,
                end: _,
                range,
            } => self.do_set_range(start, start, range.split('\n')),
            Action::SetRange {
                start,
                end,
                replaced,
            } => self.do_set_range(start, end, replaced),
            Action::InsertLines { start, count } => self.do_remove_lines(start..start + count),
            Action::Group(actions) => {
                let mut ret = Vec::with_capacity(actions.len());
                for x in actions.into_iter().rev() {
                    ret.push(self.do_undo(x));
                }
                Action::Group(ret)
            }
        }
    }

    pub fn undo(&mut self) {
        let Some(action) = self.actions.pop_back() else {
            return;
        };
        self.dirty = true;
        let action = self.do_undo(action);
        self.redo_actions.push(action);
    }

    pub fn redo(&mut self) {
        let Some(action) = self.redo_actions.pop() else {
            return;
        };
        self.dirty = true;
        let action = self.do_undo(action);
        self.actions.push_back(action);
    }
}

fn insert_in_middle<T>(mut vec: Vec<T>, idx: usize, middle: impl IntoIterator<Item = T>) -> Vec<T> {
    let iter = middle.into_iter();
    let mut ret = Vec::with_capacity(vec.len() + iter.size_hint().0);

    for r in vec.drain(..idx) {
        ret.push(r);
    }

    for l in iter {
        ret.push(l);
    }

    ret.extend(vec);

    ret
}

pub struct RangeIter<'a> {
    buf: &'a Buffer,
    start: Location,
    end: Location,
}

impl<'a> Iterator for RangeIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.start.line() > self.end.line() {
            return None;
        }
        if self.start.line() == self.end.line() {
            let ret = self.buf.get_row(self.start.line())?;
            let range = get_byte_range_from_char_range(ret, self.start.col(), self.end.col());
            let ret = &ret[range];
            self.start = Location::new(self.start.line() + 1, 0);
            return Some(ret);
        }

        let ret = self.buf.get_row(self.start.line())?;
        let idx = char_idx_to_byte_idx(ret, self.start.col()).unwrap_or(0);
        self.start = Location::new(self.start.line() + 1, 0);

        Some(&ret[idx..])
    }
}

pub struct RangeLinesIter<'a> {
    buf: &'a Buffer,
    start: usize,
    end: usize,
}

impl<'a> Iterator for RangeLinesIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.start > self.end {
            return None;
        }

        let line = self.buf.get_row(self.start)?;
        self.start += 1;
        Some(line)
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CursorDirection as C;

    #[test]
    fn buffer_read() {
        let name = "foo.vpr".to_owned();
        assert_eq!(
            Buffer::read(name.clone(), &"hello".repeat(200)),
            Buffer {
                path: Some(name.clone()),
                dirty: false,
                name,
                row: vec![Row::new("hello".repeat(200))],
                row_off: 0,
                col_off: 0,
                cur_line: 0,
                cur_col: 0,
                allow_one_past: false,
                actions: VecDeque::new(),
                redo_actions: Vec::new(),

                in_cur_group: false,
                cur_group: Vec::new(),
            }
        );
    }

    fn print_cursor(buf: &mut Buffer, rows: u16, cols: u16) {
        let (cy, cx) = buf.cursor(rows, cols);
        if let Some(row) = buf.get_row_render(cy.into(), cols.into()) {
            let idx = cx.into();
            let idx = row
                .char_indices()
                .nth(idx)
                .map(|x| x.0)
                .unwrap_or(row.len());
            let (before, after) = row.split_at(idx);
            eprintln!("{before}│{after}");
        }
    }

    type Position = (usize, usize);
    type Cursor = (u16, u16);
    fn enact(
        buf: &mut Buffer,
        rows: u16,
        cols: u16,
        actions: &[(CursorDirection, Position, Cursor)],
    ) {
        for (i, &(action, pos, cursor)) in actions.iter().enumerate() {
            buf.move_cursor(action);
            print_cursor(buf, rows, cols);
            assert_eq!(buf.position(), pos.into(), "position: #{i} {action:?}");
            assert_eq!(buf.cursor(rows, cols), cursor, "cursor: #{i} {action:?}");
        }
    }

    fn new_buf(s: &str) -> Buffer {
        let name = "foo.vpr".to_owned();
        Buffer::read(name, textwrap::dedent(s).trim())
    }

    #[test]
    fn move_cursor() {
        let rows = 24;
        let cols = 80;
        let mut buf = new_buf(
            r#"
             foo bar
             baz
             blah
             "#,
        );
        buf.set_go_past_end(true);

        enact(
            &mut buf,
            rows,
            cols,
            &[
                (C::Left, (0, 0), (0, 0)),
                (C::Right, (0, 1), (0, 1)),
                (C::Left, (0, 0), (0, 0)),
                (C::Down, (1, 0), (1, 0)),
                (C::Right, (1, 1), (1, 1)),
                (C::Right, (1, 2), (1, 2)),
                (C::Right, (1, 3), (1, 3)),
                (C::Right, (1, 3), (1, 3)),
            ],
        );
    }

    #[test]
    fn move_cursor_scroll() {
        let rows = 3;
        let cols = 80;
        let mut buf = new_buf(
            r#"
                line 1
                line 2
                line 3
                line 4
                line 5
                "#,
        );
        buf.set_go_past_end(true);

        enact(
            &mut buf,
            rows,
            cols,
            &[
                (C::Down, (1, 0), (1, 0)),
                (C::Down, (2, 0), (2, 0)),
                (C::Down, (3, 0), (2, 0)),
                (C::Down, (4, 0), (2, 0)),
                (C::Down, (4, 0), (2, 0)),
                (C::Up, (3, 0), (1, 0)),
                (C::Up, (2, 0), (0, 0)),
                (C::Up, (1, 0), (0, 0)),
                (C::Up, (0, 0), (0, 0)),
            ],
        );
    }

    #[test]
    fn move_cursor_end_of_line() {
        let rows = 3;
        let cols = 80;
        let mut buf = new_buf(
            r#"
            long line 1
            line 2
            "#,
        );
        buf.set_go_past_end(true);

        for _ in 0..11 {
            buf.move_cursor(C::Right);
        }
        assert_eq!(buf.position(), (0, 11).into());
        assert_eq!(buf.cursor(rows, cols), (0, 11));

        enact(&mut buf, rows, cols, &[(C::Down, (1, 6), (1, 6))]);
    }

    #[test]
    fn move_cursor_scroll_end_of_line() {
        let rows = 3;
        let cols = 7;
        let mut buf = new_buf(
            r#"
            long line 1
            line 2
            "#,
        );
        buf.set_go_past_end(true);

        for _ in 0..11 {
            buf.move_cursor(C::Right);
        }
        assert_eq!(buf.position(), (0, 11).into());
        assert_eq!(buf.cursor(rows, cols), (0, 7));

        enact(&mut buf, rows, cols, &[(C::Down, (1, 6), (1, 6))]);
    }

    #[test]
    fn buffer_to_string() {
        let buf = new_buf(
            "
            this
            ",
        );
        assert_eq!(buf.save(), "this\n");
    }

    #[test]
    fn insert_char() {
        let mut buf = new_buf("this");
        assert_eq!(buf.position(), (0, 0).into());
        buf.insert_char('a');
        assert_eq!(buf.save(), "athis\n");
    }

    #[test]
    fn append_char() {
        let rows = 24;
        let cols = 80;
        let mut buf = new_buf("this");
        buf.set_go_past_end(true);
        enact(
            &mut buf,
            rows,
            cols,
            &[
                (C::Right, (0, 1), (0, 1)),
                (C::Right, (0, 2), (0, 2)),
                (C::Right, (0, 3), (0, 3)),
                (C::Right, (0, 4), (0, 4)),
            ],
        );
        buf.insert_char('a');
        assert_eq!(buf.save(), "thisa\n");
    }

    #[test]
    fn append_char_scroll() {
        let rows = 24;
        let cols = 3;
        let mut buf = new_buf("this");
        buf.set_go_past_end(true);
        enact(
            &mut buf,
            rows,
            cols,
            &[
                (C::Right, (0, 1), (0, 1)),
                (C::Right, (0, 2), (0, 2)),
                (C::Right, (0, 3), (0, 2)),
                (C::Right, (0, 4), (0, 3)),
            ],
        );
        buf.insert_char('a');
        assert_eq!(buf.save(), "thisa\n");
        assert_eq!(buf.position(), (0, 5).into());
    }

    #[test]
    fn add_newline() {
        let rows = 24;
        let cols = 80;
        let mut buf = new_buf("this");
        enact(
            &mut buf,
            rows,
            cols,
            &[(C::Right, (0, 1), (0, 1)), (C::Right, (0, 2), (0, 2))],
        );
        buf.add_newline();
        assert_eq!(buf.save(), "th\nis\n");
        assert_eq!(buf.position(), (1, 0).into());
    }

    #[test]
    fn insert_tab() {
        let rows = 24;
        let cols = 80;
        let mut buf = new_buf("this");
        buf.insert_char('\t');
        assert_eq!(buf.get_row_render(0, cols.into()), Some("    this"));
        enact(&mut buf, rows, cols, &[(C::Right, (0, 2), (0, 5))]);
        buf.insert_char('\t');
        assert_eq!(buf.save(), "\tt\this\n");
        assert_eq!(buf.get_row_render(0, cols.into()), Some("    t    his"));
    }

    #[test]
    fn edit_empty_file() {
        let mut buf = new_buf("");
        assert!(!buf.is_dirty());
        buf.insert_char('h');
        assert!(buf.is_dirty());
        assert_eq!(buf.save(), "h\n");
    }

    #[test]
    fn move_cursor_with_tabs() {
        let rows = 24;
        let cols = 80;
        let mut buf = new_buf(
            "
            int main() {
            	return 0;
            }
            ",
        );
        buf.set_go_past_end(true);
        for i in 0..13 {
            buf.move_cursor(C::Right);
            print_cursor(&mut buf, rows, cols);
            assert_eq!(buf.position(), (0, (i + 1).min(12)).into());
        }
        enact(&mut buf, rows, cols, &[(C::Down, (1, 10), (1, 13))]);
    }

    #[test]
    fn render_special() {
        let buf = new_buf("\x1b");
        assert_eq!(buf.get_row_render(0, 80), Some("X1b"));
    }

    #[test]
    fn num_lines() {
        let buf = new_buf("");
        assert_eq!(buf.num_lines(), 0);
        let buf = new_buf("foo");
        assert_eq!(buf.num_lines(), 1);
        let buf = new_buf("foo\nbar");
        assert_eq!(buf.num_lines(), 2);
    }

    #[test]
    fn default() {
        let buf = Buffer::default();
        assert!(!buf.dirty);
        assert!(buf.is_empty());
        assert_eq!(buf.position(), (0, 0).into());
        assert_eq!(buf.num_lines(), 0);
        assert!(buf.path().is_none());
    }

    #[test]
    fn name() {
        let buf = Buffer::read("name".to_string(), "");
        assert_eq!(buf.name(), "name");
    }

    macro_rules! get_range_tests {
        (
            $(
                $name:ident: $buf:expr, $start:expr, $end:expr, $expected:expr
            )*
        ) =>{

            $(

                #[test]
                fn $name() {
                    let buffer = new_buf($buf);
                    assert_eq!(
                        buffer
                            .get_range($start.into(), $end.into())
                            .collect::<Vec<_>>(),
                        $expected
                    )
                }
            )*
        };
    }

    macro_rules! delete_range_tests {
        (
            $(
                $name:ident: $buf:literal @ $cursor:expr, $start:expr, $end:expr, $expected:expr, $expected_cursor:expr
            )*
        ) =>{

            $(

                #[test]
                fn $name() {
                    let mut buffer = new_buf($buf);
                    let c = $cursor;
                    buffer.set_position(c.0, c.1);
                    buffer.delete_range($start.into(), $end.into());
                    assert_eq!(
                        buffer.save(),
                        $expected
                    );
                    assert_eq!(buffer.position(), $expected_cursor.into());
                }
            )*
        };
    }

    macro_rules! insert_lines_tests {
        ($(
            $name:ident: $buf:literal, $line:expr, $lines:expr, $expected:expr
        )*) => {
            $(

                #[test]
                fn $name() {
                    let mut buffer = new_buf($buf);
                    buffer.insert_lines($line, $lines);
                    assert_eq!(
                        buffer.save(),
                        $expected
                    )

                }

            )*
        };
    }

    macro_rules! set_range_tests {
        ($(
            $name:ident: $buf:literal @ $cur_before:expr, $start:expr, $end:expr, $lines:expr, $expected:literal @ $cur_after:expr
        )*) => {
            $(

                #[test]
                fn $name() {
                    let mut buffer = new_buf($buf);
                    let before = $cur_before;
                    buffer.set_position(before.0, before.1);
                    buffer.set_range($start.into(), $end.into(), $lines);
                    assert_eq!(
                        buffer.save(),
                        $expected
                    );
                    assert_eq!(buffer.position(), $cur_after.into());

                }

            )*
        };
    }

    get_range_tests! {
        get_full_range: "hello\n\nworld", (0,0), (2,5), ["hello", "", "world"]
        get_almost_full_range: "hello\n\nworld", (0,0), (2,4), ["hello", "", "worl"]
        get_empty_range: "hello\n\nworld", (0,0), (0,0), [""]
        get_on_one_line: "hello\n\nworld", (0,1), (0,3), ["el"]
        get_invalid_range: "hello\n\nworld", (0, 1), (0, 100), ["ello"]
    }

    delete_range_tests! {
        delete_in_single_line: "hello world" @ (0,0), (0, 2), (0, 6), "heworld\n", (0,0)
        delete_in_two_lines: "hello\n world" @ (0,0), (0, 2), (1, 1), "heworld\n", (0,0)
        delete_range_crash: ".\n\nuse anyhow::anyhow;" @ (0,0), (2, 4), (2, 10), ".\n\nuse ::anyhow;\n", (0,0)
        delete_empty_range: "hello world" @ (0,0), (0, 1), (0, 1), "hello world\n", (0,0)
        delete_with_cursor_inside_range: "hello world" @ (0, 2), (0, 1), (0, 3), "hlo world\n", (0, 1)
        delete_with_cursor_inside_range_multiline: "foo\nbar\nbaz" @ (1, 1), (0, 1), (2, 1), "faz\n", (0, 1)
        delete_newline: "foo\nbar" @ (0,0), (0, 3), (1,0), "foobar\n", (0,0)
    }

    const EMPTY: [&str; 0] = [];

    insert_lines_tests! {
        set_lines_empty: "hello world", 0, EMPTY, "hello world\n"
        set_lines_empty_buf: "", 1, ["hello world"], "hello world\n"
    }

    set_range_tests! {
        set_range_delete: "hello world" @ (0, 3), (0, 1), (0, 5), EMPTY, "h world\n" @ (0, 1)
        set_range_insert_one_line: "hello world" @ (0, 5), (0, 5), (0, 5), [","], "hello, world\n" @ (0, 5)
        set_range_insert_two_lines: "hello world" @ (0, 5), (0, 5), (0, 6), [",", "..."], "hello,\n...world\n" @ (0, 5)
        set_range_insert_three_lines: "hello world" @ (0, 5), (0, 5), (0, 6), [",", "to", "this "], "hello,\nto\nthis world\n" @ (0, 5)
        set_range_multiline: "foo\nbar"             @ (0, 0), (0, 1), (1, 2), ["u"], "fur\n" @ (0, 0)
        // TODO: multi line deletion range
    }

    macro_rules! undo_tests {
        ($(
            $name:ident() $buffer:ident = $buf:literal => $e:block
        )* ) => {
            $(
                #[test]
                fn $name() {
                    let orig = if $buf.ends_with('\n') {
                        $buf
                    } else {
                        concat!($buf, "\n")
                    };
                    let mut $buffer = new_buf(orig);
                    $e;
                    let tmp = $buffer.save();
                    $buffer.undo();
                    assert_eq!($buffer.save(), orig, "undo didn't restore");
                    $buffer.redo();
                    assert_eq!($buffer.save(), tmp);
                }
            )*
        };
    }

    undo_tests! {
        undo_nothing() buffer = "alksdj" => {}
        undo_insert_char() buffer = "hello world" => {
            buffer.set_position(0, 5);
            buffer.insert_char(',');
        }
        undo_remove_line() buffer = "hello\nworld\nbar" => {
            buffer.remove_line(1);
        }
        undo_remove_lines() buffer = "foo\nbar\nbaz" => {
            buffer.remove_lines(0..2);
        }
        undo_add_newline() buffer = "foobar" => {
            buffer.set_position(0, 3);
            buffer.add_newline();
        }
        undo_add_newline2() buffer = "- [ ] abc\n- [ ] def" => {
            buffer.set_position(1, 2);
            buffer.add_newline();
            assert_eq!(buffer.save(), "- [ ] abc\n- \n[ ] def\n");
        }
        undo_delete_range1() buffer = "foobar" => {
            buffer.delete_range(Location::new(0, 2), Location::new(0, 3));
            assert_eq!(buffer.save(), "fobar\n");
        }
        undo_delete_range2() buffer = "foo\nbar" => {
            buffer.delete_range(Location::new(0, 3), Location::new(1, 0));
            assert_eq!(buffer.save(), "foobar\n");
        }
        undo_delete_range3() buffer = "foo\nbar\nbam" => {
            buffer.delete_range(Location::new(0, 2), Location::new(2, 1));
            assert_eq!(buffer.save(), "foam\n");
        }
        undo_set_range1() buffer = "hello" => {
            buffer.set_range(Location::new(0, 1), Location::new(0, 2), ["a"]);
        }
        undo_set_range2() buffer = "hello" => {
            buffer.set_range(Location::new(0, 1), Location::new(0, 2), ["ab"]);
        }
        undo_set_range3() buffer = "hello" => {
            buffer.set_range(Location::new(0, 3), Location::new(0, 3), ["", ""]);
            assert_eq!(buffer.save(), "hel\nlo\n");
        }
        undo_set_range4() buffer = "hello" => {
            buffer.set_range(Location::new(0, 2), Location::new(0, 3), ["", ""]);
        }

        undo_insert_lines1() buffer = "hello\nworld" => {
            buffer.insert_lines(0, ["a"]);
            assert_eq!(buffer.save(), "a\nhello\nworld\n");
        }
        undo_insert_lines2() buffer = "hello\nworld" => {
            buffer.insert_lines(1, ["a"]);
            assert_eq!(buffer.save(), "hello\na\nworld\n");
        }
        undo_insert_lines3() buffer = "hello\nworld" => {
            buffer.insert_lines(1, ["a", "b"]);
        }
        undo_group() buffer = "hello" => {
            buffer.start_action();
            buffer.insert_char('a');
            buffer.insert_char('b');
            buffer.insert_char('c');
            dbg!(&buffer.cur_group);
            buffer.finish_action();
            dbg!(&buffer.cur_group);
            dbg!(&buffer.actions);
        }

    }
}
