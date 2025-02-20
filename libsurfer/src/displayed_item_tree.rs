use crate::displayed_item::DisplayedItemRef;
use crate::MoveDir;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Node {
    pub item_ref: DisplayedItemRef,
    /// Nesting level of the node.
    pub level: u8,
    /// Whether a subtree of this node (if it exists) is shown
    pub unfolded: bool,
    pub selected: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MoveError {
    InvalidIndex,
    InvalidLevel,
    CircularMove,
    LevelTooDeep,
}

/// N-th visible item, becomes invalid after any add/remove/move/fold/unfold operation
#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct VisibleItemIndex(pub usize);

/// N-th item, may currently be invisible, becomes invalid after any add/remove/move operation
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
pub struct ItemIndex(pub usize);

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct TargetPosition {
    /// before which index to insert, may be in a range of 0..=tree.len() to allow for appending
    pub before: ItemIndex,
    /// at which level to insert, if None the level is derived from the item before
    pub level: u8, // TODO go back to Option and implement
}

pub struct VisibleItemIterator<'a> {
    items: &'a Vec<Node>,
    next_idx: usize,
}

impl<'a> Iterator for VisibleItemIterator<'a> {
    type Item = &'a Node;

    fn next(&mut self) -> Option<Self::Item> {
        let this_idx = self.next_idx;

        let this_item = self.items.get(this_idx);
        if this_item.is_some() {
            self.next_idx = next_visible_item(self.items, this_idx);
        };
        this_item
    }
}

#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct VisibleItemIteratorMut<'a> {
    items: &'a mut Vec<Node>,
    /// Index of the next element to return, not guaranteed to be in-bounds
    next_idx: usize,
}

impl<'a> Iterator for VisibleItemIteratorMut<'a> {
    type Item = &'a mut Node;

    fn next(&mut self) -> Option<Self::Item> {
        let this_idx = self.next_idx;

        if this_idx < self.items.len() {
            self.next_idx = next_visible_item(self.items, this_idx);

            let ptr = self.items.as_mut_ptr();
            // access is safe since we
            // - do access within bounds
            // - know that we won't generate two equal references (next call, next item)
            // - know that no second iterator or other access can happen while the references/iterator exist
            Some(unsafe { &mut *ptr.add(this_idx) })
        } else {
            None
        }
    }
}

pub struct Info<'a> {
    pub node: &'a Node,
    pub idx: ItemIndex,
    pub vidx: VisibleItemIndex,
    pub has_children: bool,
    pub last: bool,
}

pub struct VisibleItemIteratorExtraInfo<'a> {
    items: &'a Vec<Node>,
    /// Index of the next element to return, not guaranteed to be in-bounds
    next_idx: usize,
    next_vidx: usize,
}

impl<'a> Iterator for VisibleItemIteratorExtraInfo<'a> {
    type Item = Info<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let this_idx = self.next_idx;
        let this_vidx = self.next_vidx;
        if this_idx < self.items.len() {
            self.next_idx = next_visible_item(self.items, this_idx);
            self.next_vidx += 1;

            let this_level = self.items[this_idx].level;
            let has_child = self
                .items
                .get(this_idx + 1)
                .map(|item| item.level > this_level)
                .unwrap_or(false);
            Some(Info {
                node: &self.items[this_idx],
                idx: ItemIndex(this_idx),
                vidx: VisibleItemIndex(this_vidx),
                has_children: has_child,
                last: self.next_idx >= self.items.len(),
            })
        } else {
            None
        }
    }
}

/// Tree if items to be displayed
///
/// Items are stored in a flat list, with the level property indicating the nesting level
/// of the item. The items are stored in-order.
/// For documentation on the properties of a node, see the [Node] struct.
///
/// Note also infos on the [VisibleItemIndex] and [ItemIndex] types w.r.t. stability of these
/// index types.
///
/// Invariants:
/// - The nesting levels of the tree must monotonically increase (but may jump levels going down)
#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct DisplayedItemTree {
    items: Vec<Node>,
}

impl DisplayedItemTree {
    pub fn new() -> Self {
        DisplayedItemTree { items: vec![] }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Node> + use<'_> {
        self.items.iter()
    }

    /// Iterate through all visible items
    pub fn iter_visible(&self) -> VisibleItemIterator {
        VisibleItemIterator {
            items: &self.items,
            next_idx: 0,
        }
    }

    pub fn iter_visible_mut(&mut self) -> VisibleItemIteratorMut {
        VisibleItemIteratorMut {
            items: &mut self.items,
            next_idx: 0,
        }
    }

    pub fn iter_visible_extra(&self) -> VisibleItemIteratorExtraInfo {
        VisibleItemIteratorExtraInfo {
            items: &self.items,
            next_idx: 0,
            next_vidx: 0,
        }
    }

    pub fn iter_visible_selected(&self) -> impl Iterator<Item = &Node> + use<'_> {
        self.iter_visible().filter(|i| i.selected)
    }

    /// Iterate through items, skipping invisible items, return index of n-th visible item
    pub fn get_visible(&self, index: VisibleItemIndex) -> Option<&Node> {
        self.iter_visible().nth(index.0)
    }

    pub fn get_visible_extra(&self, index: VisibleItemIndex) -> Option<Info<'_>> {
        self.iter_visible_extra().nth(index.0)
    }

    pub fn get(&self, index: ItemIndex) -> Option<&Node> {
        self.items.get(index.0)
    }

    pub fn get_mut(&mut self, index: ItemIndex) -> Option<&mut Node> {
        self.items.get_mut(index.0)
    }

    pub fn to_displayed(&self, index: VisibleItemIndex) -> Option<ItemIndex> {
        self.get_visible_extra(index)?.idx.into()
    }

    /// insert item after offset visible items (either in root or in unfolded parent)
    pub fn insert_item(
        &mut self,
        item: DisplayedItemRef,
        position: TargetPosition,
    ) -> Result<ItemIndex, MoveError> {
        check_location(&self.items, position)?;

        self.items.insert(
            position.before.0,
            Node {
                item_ref: item,
                level: position.level,
                unfolded: true,
                selected: false,
            },
        );

        Ok(position.before)
    }

    /// Return the index past the end of the subtree started by `idx`
    fn subtree_end(&self, start_idx: usize) -> usize {
        let level = self.items[start_idx].level;
        self.items
            .iter()
            .skip(start_idx + 1)
            .enumerate()
            .filter_map(|(idx, x)| (x.level <= level).then_some(idx + start_idx + 1))
            .next()
            .unwrap_or(self.items.len())
    }

    pub fn remove_recursive(&mut self, ItemIndex(item): ItemIndex) -> Vec<DisplayedItemRef> {
        let end = self.subtree_end(item);
        self.items
            .drain(item..end)
            .map(|x| x.item_ref)
            .collect_vec()
    }

    pub fn remove_dissolve(&mut self, ItemIndex(item): ItemIndex) -> DisplayedItemRef {
        let end = self.subtree_end(item);
        self.items[item + 1..end]
            .iter_mut()
            .for_each(|x| x.level -= 1);
        self.items.remove(item).item_ref
    }

    pub fn drain_recursive_if<F>(&mut self, f: F) -> Vec<DisplayedItemRef>
    where
        F: Fn(&Node) -> bool,
    {
        let mut removed = vec![];

        let mut idx = 0;
        while idx < self.items.len() {
            if f(self.items.get(idx).unwrap()) {
                let end = self.subtree_end(idx);
                removed.extend(self.items.drain(idx..end).map(|x| x.item_ref));
            } else {
                idx += 1;
            }
        }

        removed
    }

    /// Find the item before `idx` that is visible, independent of level
    fn visible_predecessor(&self, mut idx: usize) -> Option<usize> {
        if idx == 0 || idx > self.items.len() {
            return None;
        }

        let start_level = self.items[idx].level;
        let mut candidate = idx - 1;
        let mut limit_level = self.items[candidate].level;

        loop {
            idx -= 1;
            let looking_item = &self.items[idx];
            // ignore subtrees deeper than what we found already
            if looking_item.level < limit_level {
                limit_level = looking_item.level;
                // the whole subtree we have been looking at is not visible,
                // assume for now the current node is
                if !looking_item.unfolded {
                    candidate = idx;
                }
            }
            if self.items[idx].level <= start_level || idx == 0 {
                return Some(candidate);
            }
        }
    }

    /// Move a visible item (and it's subtree) up/down by one visible item
    ///
    /// When moving up we move into all visible deeper trees first before skipping up.
    /// Moving down we move out until we are on the level of the next element.
    /// This way all indentations possible due to opened subtrees are reachable.
    ///
    /// Folded subtrees are skipped.
    ///
    /// `f` will be called with a node that might become the parent after move.
    /// It must return true iff that node is allowed to have child nodes.
    pub fn move_item<F>(
        &mut self,
        vidx: VisibleItemIndex,
        direction: MoveDir,
        f: F,
    ) -> Result<VisibleItemIndex, MoveError>
    where
        F: Fn(&Node) -> bool,
    {
        let Some(ItemIndex(idx)) = self.to_displayed(vidx) else {
            return Err(MoveError::InvalidIndex);
        };

        let this_level = self.items[idx].level;
        let end = self.subtree_end(idx);
        let new_index = match direction {
            MoveDir::Down => match self.items.get(end) {
                // we are at the end, but maybe still down in the hierarchy -> shift out
                None => {
                    shift_subtree_to_level(
                        &mut self.items[idx..end],
                        this_level.saturating_sub(1),
                    )?;
                    vidx
                }
                // the next node is less indented -> shift out, don't move yet
                Some(Node { level, .. }) if *level < this_level => {
                    shift_subtree_to_level(&mut self.items[idx..end], this_level - 1)?;
                    vidx
                }
                // the next node must be a sibling, it's unfolded and can have children so move into it
                Some(
                    node @ Node {
                        unfolded: true,
                        level,
                        ..
                    },
                ) if f(node) => {
                    self.move_items(
                        vec![ItemIndex(idx)],
                        TargetPosition {
                            before: ItemIndex(end + 1),
                            level: *level + 1,
                        },
                    )?;
                    VisibleItemIndex(vidx.0 + 1)
                }
                // remaining: the next node is either a folded sibling or can't have children, jump over
                _ => {
                    self.move_items(
                        vec![ItemIndex(idx)],
                        TargetPosition {
                            before: ItemIndex(self.subtree_end(end)),
                            level: this_level,
                        },
                    )?;
                    VisibleItemIndex(vidx.0 + 1)
                }
            },
            MoveDir::Up => {
                match self.visible_predecessor(idx).map(|i| (i, &self.items[i])) {
                    None => vidx,
                    // empty, unfolded node deeper/equal in, possibly to move into
                    // ... or node deeper in, but don't move into
                    Some((_node_idx, node))
                        if (node.level >= this_level && f(node) && node.unfolded)
                            | (node.level > this_level) =>
                    {
                        shift_subtree_to_level(&mut self.items[idx..end], this_level + 1)?;
                        vidx
                    }
                    Some((node_idx, node)) => {
                        self.move_items(
                            vec![ItemIndex(idx)],
                            TargetPosition {
                                before: ItemIndex(node_idx),
                                level: node.level,
                            },
                        )?;
                        VisibleItemIndex(vidx.0 - 1)
                    }
                }
            }
        };
        Ok(new_index)
    }

    /// Move multiple items to a specified location
    ///
    /// Indices may be unsorted and contain duplicates, but must be valid.
    /// Visibility is ignored for this function.
    ///
    /// Deals with any combination of items to move, except the error
    /// cases below. Rules that are followed:
    /// - The relative order of element should be the same, before and after the move
    /// - If the root of a subtree is moved, the whole subtree is moved
    /// - If a node inside a subtree is moved, then it's moved out of that subtree
    /// - If both the root and a node of a subtree are moved, the node is moved out
    ///   of the subtree and ends up after the root node
    ///
    /// Possible errors:
    /// - trying to move an element into itself
    /// - trying to move an element into another moved element
    /// - invalid indices
    /// - level too deep for subtree, level is clipped to level 255
    pub fn move_items(
        &mut self,
        indices: Vec<ItemIndex>,
        target: TargetPosition,
    ) -> Result<(), MoveError> {
        if let Some(idx) = indices.last() {
            if idx.0 >= self.items.len() {
                return Err(MoveError::InvalidIndex);
            }
        }

        // sort from back to front
        // that makes removal easier since we don't have to check whether we have to move some
        // subtree out, and the indices don't have to be updated - but moving them to the temp
        // vector needs some shuffling
        let indices = indices
            .into_iter()
            .sorted_by_key(|ii| usize::MAX - ii.0)
            .dedup()
            .collect_vec();

        let mut result = self.items.clone();
        let mut extracted = vec![];

        let mut shifted_target = target.before.0;
        for ItemIndex(start) in indices {
            let end = self.subtree_end(start);
            if ((start + 1)..end).contains(&shifted_target) {
                return Err(MoveError::CircularMove);
            }

            // if we remove elements before the target, adapt the index accordingly
            if start < shifted_target {
                shifted_target -= end - start;
            }

            shift_subtree_to_level(&mut result[start..end], target.level)?;
            extracted.splice(0..0, result.drain(start..end));
        }

        check_location(
            &result,
            TargetPosition {
                before: ItemIndex(shifted_target),
                level: target.level,
            },
        )?;

        result.splice(shifted_target..shifted_target, extracted);

        assert_eq!(self.items.len(), result.len());
        self.items = result;
        Ok(())
    }

    /// Return the range of valid levels for inserting above `item`, given the visible nodes
    ///
    /// `f` will be called with what will become the in-order predecessor node
    /// after insert. It must return true iff that node is allowed to have child nodes.
    pub fn valid_levels_visible<F>(&self, item: VisibleItemIndex, f: F) -> Range<u8>
    where
        F: Fn(&Node) -> bool,
    {
        let Some(split) = item.0.checked_sub(1) else {
            return 0..1;
        };
        match self
            .iter_visible()
            .skip(split)
            .take(2)
            .collect_vec()
            .as_slice()
        {
            [] => 0..1, // only happens for indices > self.items.len()
            [last] => {
                0..last
                    .level
                    .saturating_add(1 + (f(last) && last.unfolded) as u8)
            }
            [pre, post, ..] => {
                post.level..pre.level.saturating_add(1 + (f(pre) && pre.unfolded) as u8)
            }
        }
    }

    pub fn xfold(&mut self, ItemIndex(item): ItemIndex, unfolded: bool) {
        self.items[item].unfolded = unfolded;
        if !unfolded {
            let end = self.subtree_end(item);
            for x in &mut self.items[item..end] {
                x.selected = false;
            }
        }
    }

    pub fn xfold_all(&mut self, unfolded: bool) {
        for x in &mut self.items {
            x.unfolded = unfolded;
            if !unfolded && x.level > 0 {
                x.selected = false;
            }
        }
    }

    pub fn xfold_recursive(&mut self, ItemIndex(item): ItemIndex, unfolded: bool) {
        let end = self.subtree_end(item);
        for x in &mut self.items[item..end] {
            x.unfolded = unfolded;
            if !unfolded && x.level > 0 {
                x.selected = false;
            }
        }
    }

    // TODO should these functions fail if the item is not visible?
    pub fn xselect(&mut self, ItemIndex(item): ItemIndex, selected: bool) {
        self.items[item].selected = selected;
    }

    pub fn xselect_all(&mut self, selected: bool) {
        for x in &mut self.items {
            x.selected = selected;
        }
    }

    /// Change selection for visible items, in inclusive range
    pub fn xselect_visible_range(
        &mut self,
        VisibleItemIndex(from): VisibleItemIndex,
        VisibleItemIndex(to): VisibleItemIndex,
        selected: bool,
    ) {
        let (from, to) = if from < to {
            (from, to + 1)
        } else {
            (to, from + 1)
        };
        for node in self.iter_visible_mut().skip(from).take(to - from) {
            node.selected = selected
        }
    }

    pub fn subtree_contains(
        &self,
        ItemIndex(root): ItemIndex,
        ItemIndex(candidate): ItemIndex,
    ) -> bool {
        let end = self.subtree_end(candidate);
        (root..end).contains(&candidate)
    }
}

/// Find the index of the next visible item, or return items.len()
///
/// Precondition: `this_idx` must be a valid `items` index
fn next_visible_item(items: &[Node], this_idx: usize) -> usize {
    let this_level = items[this_idx].level;
    let mut next_idx = this_idx + 1;
    if !items[this_idx].unfolded {
        while next_idx < items.len() && items[next_idx].level > this_level {
            next_idx += 1;
        }
    }
    next_idx
}

/// Check whether `target_position` is a valid location for insertion
///
/// This means we have to check if the requested indentation level is correct.
fn check_location(items: &[Node], target_position: TargetPosition) -> Result<(), MoveError> {
    if target_position.before.0 > items.len() {
        return Err(MoveError::InvalidIndex);
    }
    let before = target_position
        .before
        .0
        .checked_sub(1)
        .and_then(|i| items.get(i));
    let after = items.get(target_position.before.0);
    let valid_range = match (before.map(|n| n.level), after.map(|n| n.level)) {
        // If we want to be the first element, no indent possible
        (None, _) => 0..=0,
        // If we want to be the last element it's allowed to be completely unindent, indent into
        // the last element, and everything in between
        (Some(before), None) => 0..=before.saturating_add(1),
        // if the latter element is indented further then the one before, we must indent
        // otherwise we'd change the parent of the after subtree
        (Some(before), Some(after)) if after > before => after..=after,
        // if before and after are at the same level, we can insert on the same level,
        // or we may indent one level
        (Some(before), Some(after)) if after == before => before..=after.saturating_add(1),
        // if before is the last element of a subtree and after is unindented
        // then we can insert anywhere between the after element (not further out bec.
        // that would change parents of the elements after), indent into the before element
        // or anything in between
        (Some(before), Some(after)) => after..=before.saturating_add(1),
    };

    if !valid_range.contains(&target_position.level) {
        Err(MoveError::InvalidLevel)
    } else {
        Ok(())
    }
}

fn shift_subtree_to_level(nodes: &mut [Node], target_level: u8) -> Result<(), MoveError> {
    let Some(from_level) = nodes.first().map(|node| node.level) else {
        return Ok(());
    };
    let level_corr = (target_level as i16) - (from_level as i16);
    for elem in nodes.iter_mut() {
        elem.level = (elem.level as i16 + level_corr)
            .try_into()
            .map_err(|_| MoveError::InvalidLevel)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use itertools::Itertools;

    fn build_tree(nodes: &[(usize, u8, bool, bool)]) -> DisplayedItemTree {
        let mut tree = DisplayedItemTree::new();
        for &(item, level, unfolded, selected) in nodes {
            tree.items.push(Node {
                item_ref: DisplayedItemRef(item),
                level,
                unfolded,
                selected,
            })
        }
        tree
    }

    /// common test tree
    /// ```text
    ///    0  1  2
    /// 0: 0
    /// 1: 1
    /// 2: 2       < folded
    /// 3:   20
    /// 4:     200
    /// 5: 3
    /// 6:   30
    /// 7:   31
    /// 8: 4
    /// 9: 5
    /// ```
    fn test_tree() -> DisplayedItemTree {
        build_tree(&[
            (0, 0, true, false),
            (1, 0, false, false),
            (2, 0, false, false),
            (20, 1, true, false),
            (200, 2, true, false),
            (3, 0, true, false),
            (30, 1, true, false),
            (31, 1, true, false),
            (4, 0, true, false),
            (5, 0, true, false),
        ])
    }

    #[test]
    fn test_iter_visible() {
        let tree = test_tree();
        assert_eq!(
            tree.iter_visible().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 1, 2, 3, 30, 31, 4, 5]
        );
    }

    #[test]
    fn test_iter_visible_extra() {
        let tree = test_tree();
        assert_eq!(
            tree.iter_visible_extra()
                .map(|info| (
                    info.node.item_ref.0,
                    info.idx.0,
                    info.vidx.0,
                    info.has_children,
                    info.last
                ))
                .collect_vec(),
            vec![
                (0, 0, 0, false, false),
                (1, 1, 1, false, false),
                (2, 2, 2, true, false),
                (3, 5, 3, true, false),
                (30, 6, 4, false, false),
                (31, 7, 5, false, false),
                (4, 8, 6, false, false),
                (5, 9, 7, false, true),
            ]
        )
    }

    #[test]
    fn test_insert_item_before_first() {
        let mut tree = test_tree();
        tree.insert_item(
            DisplayedItemRef(0xff),
            TargetPosition {
                before: ItemIndex(0),
                level: 0,
            },
        )
        .expect("insert_item must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0xff, 0, 1, 2, 20, 200, 3, 30, 31, 4, 5]
        );
        assert_eq!(tree.items[0].level, 0);
        assert_eq!(tree.items[0].selected, false);
        assert_eq!(tree.items[0].unfolded, true);
    }

    #[test]
    /// Test that inserting an element "after" the last element of a subtree
    /// does insert into the subtree, after said element
    fn test_insert_item_after_into_subtree() {
        let mut tree = test_tree();
        tree.insert_item(
            DisplayedItemRef(0xff),
            TargetPosition {
                before: ItemIndex(8),
                level: 1,
            },
        )
        .expect("insert_item must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 1, 2, 20, 200, 3, 30, 31, 0xff, 4, 5]
        );
        assert_eq!(tree.items[7].level, 1);
        assert_eq!(tree.items[7].selected, false);
        assert_eq!(tree.items[7].unfolded, true);
    }

    #[test]
    fn test_insert_item_into() {
        let mut tree = test_tree();
        tree.insert_item(
            DisplayedItemRef(0xff),
            TargetPosition {
                before: ItemIndex(7),
                level: 2,
            },
        )
        .expect("insert_item must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 1, 2, 20, 200, 3, 30, 0xff, 31, 4, 5]
        );
        assert_eq!(tree.items[7].level, 2);
        assert_eq!(tree.items[7].selected, false);
        assert_eq!(tree.items[7].unfolded, true);
    }

    #[test]
    fn test_insert_item_end() {
        let mut tree = test_tree();
        tree.insert_item(
            DisplayedItemRef(0xff),
            TargetPosition {
                before: ItemIndex(10),
                level: 0,
            },
        )
        .expect("insert_item must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 1, 2, 20, 200, 3, 30, 31, 4, 5, 0xff]
        );
        assert_eq!(tree.items[10].level, 0);
    }

    #[test]
    fn test_remove_recursive_no_children() {
        let mut tree = test_tree();
        let removed = tree.remove_recursive(ItemIndex(0));
        assert_eq!(removed, vec![DisplayedItemRef(0)]);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![1, 2, 20, 200, 3, 30, 31, 4, 5]
        );
    }

    #[test]
    fn test_remove_recursive_with_children() {
        let mut tree = test_tree();
        let removed = tree.remove_recursive(ItemIndex(2));
        assert_eq!(
            removed,
            vec![
                DisplayedItemRef(2),
                DisplayedItemRef(20),
                DisplayedItemRef(200)
            ]
        );
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 1, 3, 30, 31, 4, 5]
        );
    }

    #[test]
    fn test_remove_dissolve_with_children() {
        let mut tree = test_tree();
        let removed = tree.remove_dissolve(ItemIndex(5));
        assert_eq!(removed, DisplayedItemRef(3));
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 1, 2, 20, 200, 30, 31, 4, 5]
        );
        assert_eq!(tree.items[5].level, 0);
        assert_eq!(tree.items[6].level, 0);
    }

    #[test]
    fn test_move_item_up_unfolded_group() {
        let mut tree = build_tree(&[
            (0, 0, true, false),
            (1, 0, true, false),
            (10, 1, true, false),
            (2, 0, true, false),
            (3, 0, true, false),
        ]);
        let new_idx = tree
            .move_item(VisibleItemIndex(3), MoveDir::Up, |node| {
                node.item_ref.0 == 1
            })
            .expect("move must succeed");
        assert_eq!(new_idx.0, 3);
        assert_eq!(tree.items[3].level, 1);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 1, 10, 2, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Up, |node| node.item_ref.0 == 1)
            .expect("move must succeed");
        assert_eq!(new_idx.0, 2);
        assert_eq!(tree.items[2].level, 1);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 1, 2, 10, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Up, |node| node.item_ref.0 == 1)
            .expect("move must succeed");
        assert_eq!(new_idx.0, 1);
        assert_eq!(tree.items[1].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 2, 1, 10, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Up, |node| node.item_ref.0 == 1)
            .expect("move must succeed");
        assert_eq!(new_idx.0, 0);
        assert_eq!(tree.items[0].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![2, 0, 1, 10, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Up, |node| node.item_ref.0 == 1)
            .expect("move must succeed");
        assert_eq!(new_idx.0, 0);
        assert_eq!(tree.items[0].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![2, 0, 1, 10, 3]
        );
    }

    #[test]
    fn test_move_item_up_folded_group() {
        let mut tree = build_tree(&[
            (0, 0, true, false),
            (1, 0, false, false),
            (10, 1, true, false),
            (11, 1, true, false),
            (2, 0, true, false),
            (3, 0, true, false),
        ]);
        let new_idx = tree
            .move_item(VisibleItemIndex(2), MoveDir::Up, |node| {
                node.item_ref.0 == 1
            })
            .expect("move must succeed");
        assert_eq!(new_idx.0, 1);
        assert_eq!(tree.items[1].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 2, 1, 10, 11, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Up, |node| node.item_ref.0 == 1)
            .expect("move must succeed");
        assert_eq!(new_idx.0, 0);
        assert_eq!(tree.items[0].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![2, 0, 1, 10, 11, 3]
        );
    }

    #[test]
    fn test_move_item_down_unfolded_group() {
        let mut tree = build_tree(&[
            (0, 0, true, false),
            (1, 0, true, false),
            (2, 0, true, false),
            (20, 1, true, false),
            (3, 0, true, false),
        ]);
        let new_idx = tree
            .move_item(VisibleItemIndex(1), MoveDir::Down, |node| {
                node.item_ref.0 == 2
            })
            .expect("move must succeed");
        println!("{:?}", tree.items);
        assert_eq!(new_idx.0, 2);
        assert_eq!(tree.items[3].level, 1);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 2, 1, 20, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Down, |node| node.item_ref.0 == 2)
            .expect("move must succeed");
        println!("{:?}", tree.items);
        assert_eq!(new_idx.0, 3);
        assert_eq!(tree.items[3].level, 1);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 2, 20, 1, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Down, |node| node.item_ref.0 == 2)
            .expect("move must succeed");
        println!("{:?}", tree.items);
        assert_eq!(new_idx.0, 3);
        assert_eq!(tree.items[3].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 2, 20, 1, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Down, |node| node.item_ref.0 == 2)
            .expect("move must succeed");
        println!("{:?}", tree.items);
        assert_eq!(new_idx.0, 4);
        assert_eq!(tree.items[3].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 2, 20, 3, 1]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Down, |node| node.item_ref.0 == 2)
            .expect("move must succeed");
        println!("{:?}", tree.items);
        assert_eq!(new_idx.0, 4);
        assert_eq!(tree.items[3].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 2, 20, 3, 1]
        );
    }

    pub fn dump_tree(tree: &DisplayedItemTree) {
        let mut result = String::new();
        for (idx, node) in tree.iter().enumerate() {
            for _ in 0..node.level.saturating_sub(1) {
                result.push(' ');
            }

            if node.level > 0 {
                match tree.get(ItemIndex(idx + 1)) {
                    Some(next) if next.level < node.level => result.push_str("╰╴"),
                    _ => result.push_str("├╴"),
                }
            }

            result.push_str(&format!(
                "   ({:?}) {} {}",
                node.item_ref,
                node.unfolded.then(|| "U").unwrap_or(" "),
                node.selected.then(|| "S").unwrap_or(" ")
            ));
            if node.selected {
                result.push_str(" !SEL! ")
            }
            result.push('\n');
        }
        println!("tree: \n{}", &result);
    }

    #[test]
    fn test_move_item_down_folded_group() {
        let mut tree = build_tree(&[
            (0, 0, true, false),
            (1, 0, true, false),
            (2, 0, false, false),
            (20, 1, true, false),
            (3, 0, true, false),
        ]);
        dump_tree(&tree);
        let new_idx = tree
            .move_item(VisibleItemIndex(1), MoveDir::Down, |node| {
                node.item_ref.0 == 2
            })
            .expect("move must succeed");
        dump_tree(&tree);
        assert_eq!(new_idx.0, 2);
        assert_eq!(tree.items[3].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 2, 20, 1, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Down, |node| node.item_ref.0 == 2)
            .expect("move must succeed");
        dump_tree(&tree);
        assert_eq!(new_idx.0, 3);
        assert_eq!(tree.items[3].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 2, 20, 3, 1]
        );
    }

    #[test]
    fn test_move_items_single_to_start() {
        let mut tree = test_tree();
        tree.move_items(
            vec![ItemIndex(8)],
            TargetPosition {
                before: ItemIndex(0),
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![4, 0, 1, 2, 20, 200, 3, 30, 31, 5]
        );
        assert_eq!(tree.items[0].level, 0);
    }

    #[test]
    fn test_move_items_single_to_end() {
        let mut tree = test_tree();
        tree.move_items(
            vec![ItemIndex(4)],
            TargetPosition {
                before: ItemIndex(10),
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 1, 2, 20, 3, 30, 31, 4, 5, 200]
        );
        assert_eq!(tree.items[9].level, 0);
    }

    #[test]
    fn test_move_items_multiple_connected() {
        let mut tree = test_tree();
        tree.move_items(
            vec![ItemIndex(8), ItemIndex(9)],
            TargetPosition {
                before: ItemIndex(1),
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 4, 5, 1, 2, 20, 200, 3, 30, 31]
        );
        assert_eq!(tree.items[1].level, 0);
        assert_eq!(tree.items[2].level, 0);
    }

    #[test]
    fn test_move_items_multiple_different_levels() {
        let mut tree = test_tree();
        tree.move_items(
            vec![ItemIndex(7), ItemIndex(8)],
            TargetPosition {
                before: ItemIndex(1),
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 31, 4, 1, 2, 20, 200, 3, 30, 5]
        );
        assert_eq!(tree.items[1].level, 0);
        assert_eq!(tree.items[2].level, 0);
    }

    #[test]
    fn test_move_items_multiple_unconnected() {
        let mut tree = test_tree();
        tree.move_items(
            vec![ItemIndex(1), ItemIndex(8)],
            TargetPosition {
                before: ItemIndex(5),
                level: 1,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 2, 20, 200, 1, 4, 3, 30, 31, 5]
        );
        assert_eq!(tree.items[4].level, 1);
        assert_eq!(tree.items[5].level, 1);
    }

    #[test]
    fn test_move_items_multiple_into() {
        let mut tree = test_tree();
        tree.move_items(
            vec![ItemIndex(1), ItemIndex(8)],
            TargetPosition {
                before: ItemIndex(4),
                level: 2,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 2, 20, 1, 4, 200, 3, 30, 31, 5]
        );
        assert_eq!(tree.items[4].level, 2);
        assert_eq!(tree.items[5].level, 2);
    }

    #[test]
    fn test_move_single_to_end() {
        let mut tree = build_tree(&[
            (0, 0, false, false),
            (1, 0, false, false),
            (2, 0, false, false),
        ]);
        tree.move_items(
            vec![ItemIndex(1)],
            TargetPosition {
                before: ItemIndex(3),
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 2, 1]
        )
    }

    #[test]
    fn test_move_items_before_self_same_depth_single() {
        let ref_tree = build_tree(&[
            (0, 0, false, false),
            (1, 0, false, false),
            (2, 0, false, false),
        ]);
        let mut tree = ref_tree.clone();
        tree.move_items(
            vec![ItemIndex(1)],
            TargetPosition {
                before: ItemIndex(1),
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(tree.items, ref_tree.items);
    }

    #[test]
    fn test_move_items_after_self_same_depth_single() {
        let ref_tree = build_tree(&[
            (0, 0, false, false),
            (1, 0, false, false),
            (2, 0, false, false),
        ]);
        let mut tree = ref_tree.clone();
        tree.move_items(
            vec![ItemIndex(1)],
            TargetPosition {
                before: ItemIndex(2),
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(tree.items, ref_tree.items);
    }
    #[test]
    fn test_move_items_in_between_selected_same_depth() {
        let ref_tree = build_tree(&[
            (0, 0, false, false),
            (1, 0, false, false),
            (2, 0, false, false),
            (3, 0, false, false),
            (4, 0, false, false),
        ]);
        let mut tree = ref_tree.clone();
        tree.move_items(
            vec![ItemIndex(1), ItemIndex(2)],
            TargetPosition {
                before: ItemIndex(2),
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(tree.items, ref_tree.items);
    }

    #[test]
    /// Moving "after" a node w/o children moves nodes to the same level,
    /// so it's fine and natural that the node itself can be included in the selection
    fn test_move_items_before_self_same_depth() {
        let mut tree = test_tree();
        tree.move_items(
            vec![ItemIndex(0), ItemIndex(4), ItemIndex(9)],
            TargetPosition {
                before: ItemIndex(4),
                level: 2,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![1, 2, 20, 0, 200, 5, 3, 30, 31, 4]
        );
        assert_eq!(tree.items[3].level, 2);
        assert_eq!(tree.items[4].level, 2);
        assert_eq!(tree.items[5].level, 2);
    }

    #[test]
    /// Moving "after" a node w/o children moves nodes to the same level,
    /// so it's fine and natural that the node itself can be included in the selection
    fn test_move_items_before_self_shallower() {
        let mut tree = test_tree();
        tree.move_items(
            vec![ItemIndex(0), ItemIndex(4), ItemIndex(9)],
            TargetPosition {
                before: ItemIndex(4),
                level: 1,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![1, 2, 20, 0, 200, 5, 3, 30, 31, 4]
        );
        assert_eq!(tree.items[3].level, 1);
        assert_eq!(tree.items[4].level, 1);
        assert_eq!(tree.items[5].level, 1);
    }

    #[test]
    fn test_move_items_empty_list() {
        let mut tree = test_tree();
        tree.move_items(
            vec![],
            TargetPosition {
                before: ItemIndex(5),
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 1, 2, 20, 200, 3, 30, 31, 4, 5]
        );
        assert_eq!(tree.items, test_tree().items);
    }

    #[test]
    fn test_move_items_shared_subtree_no_overlap() {
        let mut tree = build_tree(&[
            (0, 0, true, false),
            (10, 1, false, false),
            (11, 1, false, false),
            (12, 1, false, false),
            (13, 1, false, false),
        ]);
        tree.move_items(
            vec![ItemIndex(2), ItemIndex(4)],
            TargetPosition {
                before: ItemIndex(4),
                level: 2,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item_ref.0).collect_vec(),
            vec![0, 10, 12, 11, 13]
        );
    }

    #[test]
    /// An element can't be the sub-element of itself, so we must catch that
    fn test_move_items_reject_into_self() {
        let mut tree = test_tree();
        let result = tree.move_items(
            vec![ItemIndex(2)],
            TargetPosition {
                before: ItemIndex(3),
                level: 1,
            },
        );
        assert_eq!(result, Err(MoveError::CircularMove));
        assert_eq!(tree.items, test_tree().items);
    }

    #[test]
    /// An element can't be the sub-element of itself, even not as the
    /// last element
    fn test_move_items_reject_after_self_into_subtree() {
        let mut tree = test_tree();
        let result = tree.move_items(
            vec![ItemIndex(0), ItemIndex(3), ItemIndex(9)],
            TargetPosition {
                before: ItemIndex(4),
                level: 2,
            },
        );
        assert_eq!(result, Err(MoveError::CircularMove));
        assert_eq!(tree.items, test_tree().items);
    }

    #[test]
    /// Test that the subtree check before moving is done correctly.
    /// The valid subtree element 100 also being moved prevents simpler
    /// checks (like checking only the first pre-index) from passing incorrectly.
    fn test_move_items_reject_into_subtree_distant() {
        let reference = build_tree(&[
            (1, 0, true, false),
            (10, 1, true, false),
            (100, 2, true, false),
            (11, 3, true, false),
        ]);
        let mut tree = reference.clone();
        let result = tree.move_items(
            vec![ItemIndex(1), ItemIndex(2)],
            TargetPosition {
                before: ItemIndex(4),
                level: 2,
            },
        );
        assert_eq!(result, Err(MoveError::CircularMove));
        assert_eq!(tree.items, reference.items);
    }

    #[test]
    fn test_valid_levels() {
        let tree = build_tree(&[
            /* vidx */
            /* 0 */ (0, 0, true, false),
            /* 1 */ (1, 0, true, false),
            /* 2 */ (2, 0, false, false),
            /* - */ (20, 1, true, false),
            /* 3 */ (3, 0, true, false),
            /* 4 */ (30, 1, true, false),
            /* 5 */ (300, 2, true, false),
            /* 6 */ (4, 0, true, false),
            /* 7 */ (40, 1, true, false),
            /* 8 */ (400, 2, true, false),
            /* 9 */ (41, 1, true, false),
            /* 10 */ (410, 2, true, false),
        ]);

        // To insert before the first element we can't indent,
        // regardless of what comes after
        assert_eq!(
            tree.valid_levels_visible(VisibleItemIndex(0), |_| false),
            0..1
        );
        assert_eq!(
            tree.valid_levels_visible(VisibleItemIndex(0), |_| true),
            0..1
        );

        // if flat we don't allow indent, except if the app logic allows it
        assert_eq!(
            tree.valid_levels_visible(VisibleItemIndex(1), |_| false),
            0..1
        );
        assert_eq!(
            tree.valid_levels_visible(VisibleItemIndex(1), |_| true),
            0..2
        );

        // invisible item must be ignored, do not move into (and not "loose" signal)
        assert_eq!(
            tree.valid_levels_visible(VisibleItemIndex(3), |_| false),
            0..1
        );
        assert_eq!(
            tree.valid_levels_visible(VisibleItemIndex(3), |_| true),
            0..1
        );

        // if we are past a full "cliff" allow to insert all along to the root
        assert_eq!(
            tree.valid_levels_visible(VisibleItemIndex(6), |_| false),
            0..3
        );
        assert_eq!(
            tree.valid_levels_visible(VisibleItemIndex(6), |_| true),
            0..4
        );

        // if the next item is indented then we don't allow to go to the root
        // otherwise the moved element would become the new root of some subtree
        assert_eq!(
            tree.valid_levels_visible(VisibleItemIndex(9), |_| false),
            1..3
        );
        assert_eq!(
            tree.valid_levels_visible(VisibleItemIndex(9), |_| true),
            1..4
        );

        // past the end we can go back to the root
        assert_eq!(
            tree.valid_levels_visible(VisibleItemIndex(11), |_| false),
            0..3
        );
        assert_eq!(
            tree.valid_levels_visible(VisibleItemIndex(11), |_| true),
            0..4
        );
    }
}
