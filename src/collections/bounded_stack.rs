use std::collections::VecDeque;

/// stack of bounded size, for undo/redo
#[derive(Debug, PartialEq, Eq)]
pub struct BoundedStack<T> {
    capacity: usize,
    items: VecDeque<T>,
    redo_items: Vec<T>,
}

impl<T> Default for BoundedStack<T> {
    fn default() -> Self {
        Self {
            capacity: Default::default(),
            items: Default::default(),
            redo_items: Default::default(),
        }
    }
}

impl<T> BoundedStack<T> {
    pub fn new_with_capacity(cap: usize) -> Self {
        Self {
            capacity: cap,
            items: VecDeque::new(),
            redo_items: Vec::new(),
        }
    }

    pub fn push(&mut self, item: T) {
        self.redo_items.clear();
        if self.items.len() == self.capacity {
            self.items.pop_front();
        }
        self.items.push_back(item);
    }

    /// applies `f` to the last pushed item and keeps track of it to redo
    ///
    /// does nothing if the stack is empty
    pub fn undo(&mut self, f: impl FnOnce(T) -> T) {
        let Some(top) = self.items.pop_back() else {
            return;
        };

        let top = f(top);

        self.redo_items.push(top);
    }

    /// applies `f` to the last undone item and keeps track of it to undo
    ///
    /// does nothing if the stack is empty
    pub fn redo(&mut self, f: impl FnOnce(T) -> T) {
        let Some(top) = self.redo_items.pop() else {
            return;
        };

        let top = f(top);

        self.items.push_back(top);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct Call(Cell<bool>);
    impl Call {
        fn new() -> Self {
            Call(Cell::new(false))
        }

        fn did_call<T>(&self, x: T) -> T {
            self.0.set(true);
            x
        }

        #[track_caller]
        fn assert_call(&self) {
            assert!(self.0.get(), "function wasn't called");
            self.0.set(false)
        }
    }

    #[test]
    fn it_works() {
        let mut s = BoundedStack::new_with_capacity(3);
        s.push(1);
        s.push(2);
        s.push(3);
        s.push(4);
        let call = Call::new();
        s.undo(|x| {
            assert_eq!(x, 4);
            call.did_call(x)
        });
        call.assert_call();

        s.undo(|x| {
            assert_eq!(x, 3);
            call.did_call(x)
        });
        call.assert_call();

        s.undo(|x| {
            assert_eq!(x, 2);
            call.did_call(x)
        });
        call.assert_call();

        s.undo(|_| panic!());
    }

    #[test]
    fn redo_works() {
        let mut s = BoundedStack::new_with_capacity(3);
        s.push(1);
        s.push(2);
        s.push(3);
        s.push(4);
        let call = Call::new();
        s.undo(|x| {
            assert_eq!(x, 4);
            call.did_call(x)
        });
        call.assert_call();

        s.redo(|x| {
            assert_eq!(x, 4);
            call.did_call(x)
        });
        call.assert_call();
    }

    #[test]
    fn push_onto_undone() {
        let mut s = BoundedStack::new_with_capacity(3);
        s.push(1);
        s.push(2);
        s.push(3);
        s.push(4);
        let call = Call::new();
        s.undo(|x| {
            assert_eq!(x, 4);
            call.did_call(x)
        });
        call.assert_call();

        s.push(5);

        s.redo(|_| panic!());
    }
}
