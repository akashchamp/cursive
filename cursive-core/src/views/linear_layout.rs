/// Event fired when the view is about to lose focus.
use crate::{
    Printer, Vec2, With, XY, direction,
    event::{AnyCb, Event, EventResult, Key},
    rect::Rect,
    view::{
        CannotFocus, IntoBoxedView, Selector, SizeCache, View, ViewNotFound,
        layout_pass::{current_pass, in_layout_pass},
    },
};
use log::debug;
use std::cmp::min;
use std::ops::Deref;

/// Arranges its children linearly according to its orientation.
///
/// # Examples
///
/// ```
/// use cursive_core::traits::Resizable;
/// use cursive_core::views::{Button, LinearLayout, TextArea, TextView};
///
/// let linear_layout = LinearLayout::horizontal()
///     .child(TextView::new("Top of the page"))
///     .child(TextArea::new().fixed_size((20, 5)))
///     .child(Button::new("Ok", |s| s.quit()));
/// ```
pub struct LinearLayout {
    children: Vec<Child>,
    orientation: direction::Orientation,
    focus: usize,

    cache: Option<XY<SizeCache>>,

    // Every size computed during the current layout pass (see `memo_pass`).
    //
    // A parent measures each child up to 3 times with different constraints,
    // so with `cache` alone, nested layouts would redo the work of their
    // whole subtree at every level: exponential in depth.
    memo: Vec<Memo>,
    // The layout pass `memo` was filled during.
    memo_pass: Option<u64>,
}

// The result of a `required_size` computation, with what it left behind.
struct Memo {
    req: Vec2,
    size: Vec2,
    cache: Option<XY<SizeCache>>,
    children: Vec<Vec2>,
}

struct Child {
    view: Box<dyn View>,

    // The last result from the child's required_size
    // Doesn't have to be what the child actually gets.
    required_size: Vec2,

    last_size: Vec2,

    _weight: usize,
}

impl Child {
    // Compute and caches the required size.
    fn required_size(&mut self, req: Vec2) -> Vec2 {
        self.required_size = self.view.required_size(req);
        self.required_size
    }

    fn layout(&mut self, size: Vec2) {
        self.last_size = size;
        self.view.layout(size);
    }

    fn as_view(&self) -> &dyn View {
        &*self.view
    }
}

struct ChildIterator<I> {
    // Actual iterator on the children
    inner: I,
    // Current offset
    offset: usize,
    // Available size
    available: usize,
    // Orientation for this layout
    orientation: direction::Orientation,
}

struct ChildItem<T> {
    child: T,
    offset: usize,
    length: usize,
}

impl<T> ChildIterator<T> {
    fn new(inner: T, orientation: direction::Orientation, available: usize) -> Self {
        ChildIterator {
            inner,
            available,
            orientation,
            offset: 0,
        }
    }
}

impl<T: Deref<Target = Child>, I: Iterator<Item = T>> Iterator for ChildIterator<I> {
    type Item = ChildItem<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|child| {
            // Save the current offset.
            let offset = self.offset;

            // debug!("Available: {}", self.available);

            let length = min(self.available, *child.required_size.get(self.orientation));

            // Allocated width
            self.available = self.available.saturating_sub(length);

            self.offset += length;

            ChildItem {
                child,
                offset,
                length,
            }
        })
    }
}

fn cap<'a, I: Iterator<Item = &'a mut usize>>(iter: I, max: usize) {
    let mut available = max;
    for item in iter {
        if *item > available {
            *item = available;
        }

        available -= *item;
    }
}

impl LinearLayout {
    /// Creates a new layout with the given orientation.
    pub fn new(orientation: direction::Orientation) -> Self {
        LinearLayout {
            children: Vec::new(),
            orientation,
            focus: 0,
            cache: None,
            memo: Vec::new(),
            memo_pass: None,
        }
    }

    /// Sets the weight of the given child. This weight is currently unused by the layout process.
    ///
    /// # Panics
    ///
    /// Panics if `i >= self.len()`.
    pub fn set_weight(&mut self, i: usize, weight: usize) {
        self.children[i]._weight = weight;
    }

    /// Modifies the weight of the last child added. This weight is currently unused by the layout process.
    ///
    /// It is an error to call this before adding a child (and it will panic).
    #[must_use]
    pub fn weight(mut self, weight: usize) -> Self {
        self.children.last_mut().unwrap()._weight = weight;

        self
    }

    /// Adds a child to the layout.
    ///
    /// Chainable variant.
    #[must_use]
    pub fn child<V: IntoBoxedView + 'static>(self, view: V) -> Self {
        self.with(|s| s.add_child(view))
    }

    /// Adds a child to the layout.
    pub fn add_child<V: IntoBoxedView + 'static>(&mut self, view: V) {
        self.children.push(Child {
            view: view.into_boxed_view(),
            required_size: Vec2::zero(),
            last_size: Vec2::zero(),
            _weight: 0,
        });
        self.invalidate();
    }

    /// Inserts a child at the given position.
    ///
    /// # Panics
    ///
    /// Panics if `i > self.len()`.
    pub fn insert_child<V: IntoBoxedView + 'static>(&mut self, i: usize, view: V) {
        self.children.insert(
            i,
            Child {
                view: view.into_boxed_view(),
                required_size: Vec2::zero(),
                last_size: Vec2::zero(),
                _weight: 0,
            },
        );
        self.invalidate();
    }

    /// Swaps two children.
    pub fn swap_children(&mut self, i: usize, j: usize) {
        self.children.swap(i, j);
        // No need to invalidate, total size should be the same.
    }

    /// Returns the number of children.
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Returns `true` if this view has no children.
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Returns index of focused inner view
    pub fn get_focus_index(&self) -> usize {
        self.focus
    }

    /// Attempts to set the focus on the given child.
    ///
    /// Returns `Err(ViewNotFound)` if `index >= self.len()`, or if the view at the
    /// given index does not accept focus.
    pub fn set_focus_index(&mut self, index: usize) -> Result<EventResult, ViewNotFound> {
        self.children
            .get_mut(index)
            .and_then(|child| child.view.take_focus(direction::Direction::none()).ok())
            .map(|res| res.and(self.set_focus_unchecked(index)))
            .ok_or(ViewNotFound)
    }

    fn set_focus_unchecked(&mut self, index: usize) -> EventResult {
        if index != self.focus {
            let result = self.children[self.focus].view.on_event(Event::FocusLost);
            self.focus = index;
            result
        } else {
            EventResult::Consumed(None)
        }
    }

    // Invalidate the view, to request a layout next time
    fn invalidate(&mut self) {
        self.cache = None;
        self.memo.clear();
    }

    /// Creates a new vertical layout.
    pub fn vertical() -> Self {
        LinearLayout::new(direction::Orientation::Vertical)
    }

    /// Creates a new horizontal layout.
    pub fn horizontal() -> Self {
        LinearLayout::new(direction::Orientation::Horizontal)
    }

    /// Returns a reference to a child.
    pub fn get_child(&self, i: usize) -> Option<&dyn View> {
        self.children.get(i).map(|child| &*child.view)
    }

    /// Returns a mutable reference to a child.
    pub fn get_child_mut(&mut self, i: usize) -> Option<&mut dyn View> {
        // Anything could happen to the child view, so bust the cache.
        self.invalidate();
        self.children.get_mut(i).map(|child| &mut *child.view)
    }

    /// Removes all children from this view.
    pub fn clear(&mut self) {
        self.invalidate();
        self.children.clear();
        self.focus = 0;
    }

    /// Removes a child.
    ///
    /// If `i` is within bounds, the removed child will be returned.
    pub fn remove_child(&mut self, i: usize) -> Option<Box<dyn View>> {
        if i < self.children.len() {
            // Any alteration means we should invalidate the cache.
            self.invalidate();

            // Keep the same view focused.
            if self.focus > i || (self.focus != 0 && self.focus == self.children.len() - 1) {
                self.focus -= 1;
            }

            // Return the wrapped view
            Some(self.children.remove(i).view)
        } else {
            // This includes empty list
            None
        }
    }

    /// Looks for the child containing a view with the given name.
    ///
    /// Returns `Some(i)` if `self.get_child(i)` has the given name, or
    /// contains a view with the given name.
    ///
    /// Returns `None` if the given name was not found.
    pub fn find_child_from_name(&mut self, name: &str) -> Option<usize> {
        let selector = Selector::Name(name);
        for (i, c) in self.children.iter_mut().enumerate() {
            let mut found = false;
            c.view.call_on_any(&selector, &mut |_| found = true);
            if found {
                return Some(i);
            }
        }
        None
    }

    // If the cache can be used, return the cached size.
    // Otherwise, return None.
    fn get_cache(&self, req: Vec2) -> Option<Vec2> {
        match self.cache {
            None => None,
            Some(ref cache) => {
                // Is our cache even valid?
                // Also, is any child invalidating the layout?
                if cache.zip_map(req, SizeCache::accept).both() && self.children_are_sleeping() {
                    Some(cache.map(|s| s.value))
                } else {
                    None
                }
            }
        }
    }

    fn children_are_sleeping(&self) -> bool {
        !self
            .children
            .iter()
            .map(Child::as_view)
            .any(View::needs_relayout)
    }

    /// Returns a mutable iterator starting with the child in focus
    fn iter_mut<'a>(
        &'a mut self,
        from_focus: bool,
        source: direction::Relative,
    ) -> Box<dyn Iterator<Item = (usize, &'a mut Child)> + 'a> {
        match source {
            direction::Relative::Front => {
                let start = if from_focus { self.focus } else { 0 };

                Box::new(self.children.iter_mut().enumerate().skip(start))
            }
            direction::Relative::Back => {
                let end = if from_focus {
                    self.focus + 1
                } else {
                    self.children.len()
                };
                Box::new(self.children[..end].iter_mut().enumerate().rev())
            }
        }
    }

    // Attempt to move the focus, coming from the given direction.
    //
    // Consumes the event if the focus was moved, otherwise ignores it.
    fn move_focus(&mut self, source: direction::Direction) -> EventResult {
        source
            .relative(self.orientation)
            .and_then(|rel| {
                // The iterator starts at the focused element.
                // We don't want that one.
                self.iter_mut(true, rel)
                    .skip(1)
                    .find_map(|p| try_focus(p, source))
            })
            .map_or(EventResult::Ignored, |(i, res)| {
                res.and(self.set_focus_unchecked(i))
            })
    }

    // Move the focus to the selected view if needed.
    //
    // Does nothing if the event is not a `MouseEvent`.
    fn check_focus_grab(&mut self, event: &Event) -> Option<EventResult> {
        if let Event::Mouse {
            offset,
            position,
            event,
        } = *event
        {
            if !event.grabs_focus() {
                return None;
            }

            let position = position.checked_sub(offset)?;

            // Find the selected child
            // Let's only care about the coordinate for our orientation.
            let position = *position.get(self.orientation);

            // Iterate on the views and find the one
            // We need a mutable ref to call take_focus later on.
            for (i, item) in ChildIterator::new(
                self.children.iter_mut(),
                self.orientation,
                // TODO: get actual width (not super important)
                usize::MAX,
            )
            .enumerate()
            {
                // Get the child size:
                // this will give us the allowed window for a click.
                let child_size = item.child.last_size.get(self.orientation);

                if item.offset + child_size <= position {
                    continue;
                }

                return item
                    .child
                    .view
                    .take_focus(direction::Direction::none())
                    .ok()
                    .map(|res| res.and(self.set_focus_unchecked(i)));
            }
        }
        None
    }

    fn layout_in_pass(&mut self, size: Vec2) {
        if self.get_cache(size).is_none() {
            // Build the cache if needed.
            self.required_size(size);
        }

        // We'll use this guy a few times, but it's a mouthful...
        let o = self.orientation;

        for item in ChildIterator::new(self.children.iter_mut(), o, *size.get(o)) {
            // Every item has the same size orthogonal to the layout
            let size = size.with_axis(o, item.length);

            item.child.layout(size);
        }
    }

    // The actual size negotiation, without any caching.
    fn compute_required_size(&mut self, req: Vec2) -> Vec2 {
        debug!("Req: {:?}", req);

        // First, make a naive scenario: everything will work fine.
        let ideal_sizes: Vec<Vec2> = self
            .children
            .iter_mut()
            .map(|c| c.required_size(req))
            .collect();
        debug!("Ideal sizes: {:?}", ideal_sizes);
        let ideal = self.orientation.stack(ideal_sizes.iter().copied());
        debug!("Ideal result: {:?}", ideal);

        // Does it fit?
        if ideal.fits_in(req) {
            // Champagne!
            self.cache = Some(SizeCache::build(ideal, req));
            return ideal;
        }

        // Ok, so maybe it didn't. Budget cuts, everyone.
        // Let's pretend we have almost no space in this direction.
        // budget_req is the dummy requirements, in an extreme budget
        // situation.
        let budget_req = req.with_axis(self.orientation, 1);
        debug!("Budget req: {:?}", budget_req);

        // See how they like it that way.
        // This is, hopefully, the absolute minimum these views will accept.
        let min_sizes: Vec<Vec2> = self
            .children
            .iter_mut()
            .map(|c| c.required_size(budget_req))
            .collect();
        let desperate = self.orientation.stack(min_sizes.iter().copied());
        debug!("Min sizes: {:?}", min_sizes);
        debug!("Desperate: {:?}", desperate);

        // This is the lowest we'll ever go. It better fit at least.
        let orientation = self.orientation;
        if desperate.get(orientation) > req.get(orientation) {
            // Just give up...
            // TODO: hard-cut
            cap(
                self.children
                    .iter_mut()
                    .map(|c| c.required_size.get_mut(orientation)),
                *req.get(self.orientation),
            );

            // TODO: print some error message or something
            debug!("Seriously? {:?} > {:?}???", desperate, req);
            // self.cache = Some(SizeCache::build(desperate, req));
            self.cache = None;
            return desperate;
        }

        // So now that we know we _can_ make it all fit, we can redistribute
        // the extra space we have.

        // This here is how much we're generously offered
        // (We just checked that req >= desperate, so the subtraction is safe
        let mut available = self.orientation.get(&(req.saturating_sub(desperate)));
        debug!("Available: {:?}", available);

        // Here, we have to make a compromise between the ideal
        // and the desperate solutions.
        // This is the vector of (ideal - minimum) sizes for each view.
        // (which is how much they would like to grow)
        let mut overweight: Vec<(usize, usize)> = ideal_sizes
            .iter()
            .map(|v| self.orientation.get(v))
            .zip(min_sizes.iter().map(|v| self.orientation.get(v)))
            .map(|(a, b)| a.saturating_sub(b))
            .enumerate()
            .collect();
        debug!("Overweight: {:?}", overweight);

        // So... distribute `available` to reduce the overweight...
        // TODO: use child weight in the distribution...

        // We'll give everyone his share of what we have left,
        // starting with those who ask the least.
        overweight.sort_by_key(|&(_, weight)| weight);
        let mut allocations = vec![0; overweight.len()];

        for (i, &(j, weight)) in overweight.iter().enumerate() {
            // This is the number of people we still have to feed.
            let remaining = overweight.len() - i;
            // How much we can spare on each one
            let budget = available / remaining;
            // Maybe he doesn't even need that much?
            let spent = min(budget, weight);
            allocations[j] = spent;
            available -= spent;
        }
        debug!("Allocations: {:?}", allocations);

        // Final lengths are the minimum ones + generous allocations
        let final_lengths: Vec<Vec2> = min_sizes
            .iter()
            .map(|v| self.orientation.get(v))
            .zip(allocations.iter())
            .map(|(a, b)| a + b)
            .map(|l| req.with_axis(self.orientation, l))
            .collect();
        debug!("Final sizes: {:?}", final_lengths);

        // Let's ask everyone one last time. Everyone should be happy.
        // (But they may ask more on the other axis.)
        let final_sizes: Vec<Vec2> = self
            .children
            .iter_mut()
            .enumerate()
            .map(|(i, c)| c.required_size(final_lengths[i]))
            .collect();
        debug!("Final sizes2: {:?}", final_sizes);

        // Let's stack everything to see what it looks like.
        let compromise = self.orientation.stack(final_sizes.iter().copied());

        // Phew, that was a lot of work! I'm not doing it again.
        self.cache = Some(SizeCache::build(compromise, req));

        compromise
    }
}

fn try_focus(
    (i, child): (usize, &mut Child),
    source: direction::Direction,
) -> Option<(usize, EventResult)> {
    child.view.take_focus(source).ok().map(|res| (i, res))
}

impl View for LinearLayout {
    fn draw(&self, printer: &Printer) {
        // Use pre-computed sizes
        // debug!("Pre loop!");
        for (i, item) in ChildIterator::new(
            self.children.iter(),
            self.orientation,
            *printer.size.get(self.orientation),
        )
        .enumerate()
        {
            // debug!("Printer size: {:?}", printer.size);
            // debug!("Child size: {:?}", item.child.required_size);
            // debug!("Offset: {:?}", item.offset);
            let printer = &printer
                .offset(self.orientation.make_vec(item.offset, 0))
                .cropped(item.child.last_size)
                .focused(i == self.focus);
            item.child.view.draw(printer);
        }
    }

    fn needs_relayout(&self) -> bool {
        if self.cache.is_none() {
            return true;
        }

        !self.children_are_sleeping()
    }

    fn layout(&mut self, size: Vec2) {
        // Children layouts will measure their own children again: keep the
        // pass open so they can reuse what was computed for them already.
        in_layout_pass(|| self.layout_in_pass(size));
    }

    fn required_size(&mut self, req: Vec2) -> Vec2 {
        // Did anything change since last time?
        if let Some(size) = self.get_cache(req) {
            return size;
        }

        in_layout_pass(|| {
            if self.memo_pass != current_pass() {
                self.memo.clear();
                self.memo_pass = current_pass();
            }

            if let Some(memo) = self.memo.iter().find(|memo| memo.req == req) {
                // Restore what `layout()` will rely on.
                for (child, &size) in self.children.iter_mut().zip(&memo.children) {
                    child.required_size = size;
                }
                self.cache = memo.cache;
                return memo.size;
            }

            let size = self.compute_required_size(req);
            self.memo.push(Memo {
                req,
                size,
                cache: self.cache,
                children: self.children.iter().map(|c| c.required_size).collect(),
            });
            size
        })
    }

    fn take_focus(&mut self, source: direction::Direction) -> Result<EventResult, CannotFocus> {
        // In what order will we iterate on the children?
        let rel = source.relative(self.orientation);

        // We activate from_focus only if coming from the "sides".
        let focus_res = self
            .iter_mut(rel.is_none(), rel.unwrap_or(direction::Relative::Front))
            .find_map(|p| try_focus(p, source));

        if let Some((next_focus, res)) = focus_res {
            // No "FocusLost" here, since we didn't have focus before.
            self.focus = next_focus;
            Ok(res)
        } else {
            Err(CannotFocus)
        }
    }

    fn on_event(&mut self, event: Event) -> EventResult {
        if self.is_empty() {
            return EventResult::Ignored;
        }

        let res = self
            .check_focus_grab(&event)
            .unwrap_or(EventResult::Ignored);

        let result = {
            let mut iterator =
                ChildIterator::new(self.children.iter_mut(), self.orientation, usize::MAX);
            let item = iterator.nth(self.focus).unwrap();
            let offset = self.orientation.make_vec(item.offset, 0);
            item.child.view.on_event(event.relativized(offset))
        };
        res.and(match result {
            EventResult::Ignored => match event {
                Event::Shift(Key::Tab) if self.focus > 0 => {
                    self.move_focus(direction::Direction::back())
                }
                Event::Key(Key::Tab) if self.focus + 1 < self.children.len() => {
                    self.move_focus(direction::Direction::front())
                }
                Event::Key(Key::Left)
                    if self.orientation == direction::Orientation::Horizontal && self.focus > 0 =>
                {
                    self.move_focus(direction::Direction::right())
                }
                Event::Key(Key::Up)
                    if self.orientation == direction::Orientation::Vertical && self.focus > 0 =>
                {
                    self.move_focus(direction::Direction::down())
                }
                Event::Key(Key::Right)
                    if self.orientation == direction::Orientation::Horizontal
                        && self.focus + 1 < self.children.len() =>
                {
                    self.move_focus(direction::Direction::left())
                }
                Event::Key(Key::Down)
                    if self.orientation == direction::Orientation::Vertical
                        && self.focus + 1 < self.children.len() =>
                {
                    self.move_focus(direction::Direction::up())
                }
                _ => EventResult::Ignored,
            },
            res => res,
        })
    }

    fn call_on_any(&mut self, selector: &Selector, callback: AnyCb) {
        for child in &mut self.children {
            child.view.call_on_any(selector, callback);
        }
    }

    fn focus_view(&mut self, selector: &Selector) -> Result<EventResult, ViewNotFound> {
        for (i, child) in self.children.iter_mut().enumerate() {
            if let Ok(res) = child.view.focus_view(selector) {
                return Ok(res.and(self.set_focus_unchecked(i)));
            }
        }

        Err(ViewNotFound)
    }

    fn important_area(&self, size: Vec2) -> Rect {
        if self.is_empty() {
            // Return dummy area if we are empty.
            return Rect::from_size(Vec2::zero(), size);
        }

        // Pick the focused item, with its offset
        let item = {
            let mut iterator =
                ChildIterator::new(self.children.iter(), self.orientation, usize::MAX);
            iterator.nth(self.focus).unwrap()
        };

        // Make a vector offset from the scalar value
        let offset = self.orientation.make_vec(item.offset, 0);

        // And ask the child its own area.
        let rect = item.child.view.important_area(item.child.last_size);

        // Add `offset` to the rect.
        rect + offset
    }
}

/*
#[crate::blueprint(LinearLayout::new(orientation))]
struct Blueprint {
    orientation: direction::Orientation,

    #[blueprint(foreach=add_child)]
    children: Vec<crate::views::BoxedView>,

    #[blueprint(
        set_focus_index,
        on_err="LinearLayout.focus cannot be larger than the number of views.",
    )]
    focus: Option<usize>,
}
*/

crate::manual_blueprint!(LinearLayout, |config, context| {
    let orientation = match config.get("orientation") {
        Some(orientation) => context.resolve(orientation)?,
        None => direction::Orientation::Vertical,
    };

    let mut layout = LinearLayout::new(orientation);

    let children: Option<Vec<crate::views::BoxedView>> =
        context.resolve_as_config(&config["children"])?;
    if let Some(children) = children {
        for child in children {
            layout.add_child(child);
        }
    }

    if let Some(focus) = config.get("focus") {
        let focus = context.resolve(focus)?;
        layout
            .set_focus_index(focus)
            .map_err(|_| crate::builder::Error::InvalidConfig {
                message: "LinearLayout.focus cannot be larger than the number of views.".into(),
                config: config.clone(),
            })?;
    }

    Ok(layout)
});

#[cfg(test)]
mod tests {
    use crate::{
        Printer, Vec2,
        view::{Nameable, Resizable, View},
        views::{DummyView, LinearLayout, Panel, TextContent, TextView},
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counts `required_size` calls, and always needs a relayout.
    struct CountingView {
        calls: Arc<AtomicUsize>,
    }

    impl View for CountingView {
        fn draw(&self, _: &Printer) {}

        fn needs_relayout(&self) -> bool {
            true
        }

        fn required_size(&mut self, req: Vec2) -> Vec2 {
            self.calls.fetch_add(1, Ordering::Relaxed);
            // Shrinkable, so enclosing layouts have something to compress.
            Vec2::new(req.x.min(4), 1)
        }
    }

    /// Nested horizontal layouts, each level wrapped by `wrap` and compressed.
    fn wrapped_chain(
        depth: usize,
        calls: &Arc<AtomicUsize>,
        wrap: fn(LinearLayout) -> Box<dyn View>,
    ) -> Box<dyn View> {
        if depth == 0 {
            return Box::new(CountingView {
                calls: calls.clone(),
            });
        }
        wrap(
            LinearLayout::horizontal()
                .child(DummyView.fixed_size((6usize, 1usize)))
                .child(wrapped_chain(depth - 1, calls, wrap)),
        )
    }

    #[test]
    fn nested_compressed_layouts_stay_linear() {
        // Each level measures its child 3 times with different constraints.
        // Without memoization that is ~4x more leaf calls per level, even
        // though the leaf only ever sees a handful of distinct constraints.
        // (name, wrapper, width each level adds besides the leaf)
        let wraps: [(&str, fn(LinearLayout) -> Box<dyn View>, usize); 3] = [
            ("bare", |l| Box::new(l), 6),
            ("named", |l| Box::new(l.with_name("level")), 6),
            ("panel", |l| Box::new(Panel::new(l)), 8),
        ];
        let depth = 10;
        for (name, wrap, level_width) in wraps {
            let calls = Arc::new(AtomicUsize::new(0));
            let mut tree = wrapped_chain(depth, &calls, wrap);
            // 2 short of the ideal width, so every level compresses.
            let size = tree.required_size(Vec2::new(level_width * depth + 2, 100));
            assert_eq!(size.x, level_width * depth + 2, "{name}");
            let measure = calls.swap(0, Ordering::Relaxed);
            assert!(measure <= 4 * depth, "{name}: {measure} leaf calls");

            // Laying out measures every level again: still linear.
            tree.layout(size);
            let layout = calls.load(Ordering::Relaxed);
            assert!(layout <= 4 * depth, "{name}: {layout} leaf calls in layout");
        }
    }

    #[test]
    fn memo_does_not_outlive_the_call() {
        // The leaf changes between two separate measurements: the second
        // one must not reuse sizes memoized by the first.
        let make = |content: TextContent| {
            LinearLayout::vertical()
                .child(TextView::new_with_content(content))
                .child(TextView::new("x"))
        };
        let req = Vec2::new(20, 10);

        let content = TextContent::new("abc");
        let mut layout = make(content.clone());
        let before = layout.required_size(req);

        content.set_content("abcdef ghijkl mnopqr stuvwx");
        let expected = make(TextContent::new("abcdef ghijkl mnopqr stuvwx")).required_size(req);
        assert_ne!(before, expected);
        assert_eq!(layout.required_size(req), expected);
    }
}
