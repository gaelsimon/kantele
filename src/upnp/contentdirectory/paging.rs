//! The page window a `Browse` asks for.

use crate::upnp::didl;

/// What one answer carries where the client named no count. Zero means "every child" on the wire,
/// and a library of tens of thousands of tracks is not an answer a renderer can hold: the total
/// still says how many there are, so a client that wants the rest asks for it.
const MOST_AT_ONCE: usize = 2_000;

/// A count of zero means every child.
#[derive(Clone, Copy, Debug)]
pub(super) struct Window {
    from: usize,
    count: usize,
}

impl Window {
    pub(super) fn new(from: usize, count: usize) -> Self {
        let count = if count == 0 { MOST_AT_ONCE } else { count };
        Self { from, count }
    }

    pub(super) const ALL: Self = Self { from: 0, count: 0 };
    /// A start past any end: an empty page beside a true total.
    pub(super) const NONE: Self = Self {
        from: usize::MAX,
        count: 0,
    };

    pub(super) fn of<T>(self, items: &[T]) -> &[T] {
        &items[self.range(items.len())]
    }

    pub(super) fn range(self, total: usize) -> std::ops::Range<usize> {
        let open = self.open_range();
        open.start.min(total)..open.end.min(total)
    }

    pub(super) fn open_range(self) -> std::ops::Range<usize> {
        let to = if self.count == 0 {
            usize::MAX
        } else {
            self.from.saturating_add(self.count)
        };
        self.from..to
    }
}

pub(super) enum Listing<'a> {
    All(Vec<didl::Child<'a>>),
    Page(Vec<didl::Child<'a>>, usize),
}

impl<'a> Listing<'a> {
    pub(super) fn page(&self, window: Window) -> (&[didl::Child<'a>], usize) {
        match self {
            Self::All(all) => (window.of(all), all.len()),
            Self::Page(page, total) => (page.as_slice(), *total),
        }
    }

    pub(super) fn total(&self) -> usize {
        match self {
            Self::All(all) => all.len(),
            Self::Page(_, total) => *total,
        }
    }

    pub(super) fn into_all(self) -> Vec<didl::Child<'a>> {
        match self {
            Self::All(all) | Self::Page(all, _) => all,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(from: usize, count: usize) -> Window {
        Window::new(from, count)
    }

    #[test]
    fn requested_count_zero_means_everything_it_can_carry() {
        let items = [1, 2, 3, 4, 5];
        assert_eq!(window(0, 0).of(&items), &items[..]);
        assert_eq!(window(2, 0).of(&items), &items[2..]);
        assert_eq!(
            window(0, 0).open_range(),
            0..MOST_AT_ONCE,
            "a library of tens of thousands of tracks is not one answer, and the total still says \
             how many there are"
        );
        assert_eq!(
            Window::ALL.open_range(),
            0..usize::MAX,
            "what this server asks itself for is not capped"
        );
    }

    #[test]
    fn paging_past_the_end_is_empty_rather_than_an_error() {
        let items = [1, 2, 3];
        assert!(window(99, 10).of(&items).is_empty());
        assert_eq!(window(1, 99).of(&items), &items[1..]);
        assert!(Window::NONE.of(&items).is_empty());
    }

    #[test]
    fn a_window_over_a_list_still_being_produced_has_no_end_until_a_count_is_asked() {
        assert_eq!(Window::ALL.open_range(), 0..usize::MAX);
        assert_eq!(window(30, 5).open_range(), 30..35);
        assert_eq!(window(30, 5).range(32), 30..32);
    }
}
