use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::ops::Range;

use crate::displayed_item::DisplayedItemRef;
use crate::MoveDir;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Node {
    pub item: DisplayedItemRef,
    /// Nesting level of the node.
    pub level: u8,
    /// Whether a subtree of this node (if it exists) is shown
    pub unfolded: bool,
    pub selected: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MoveResult {
    InvalidIndex,
    InvalidLevel,
    CircularMove,
}

/// N-th visible item, becomes invalid after any add/remove/move/fold/unfold operation
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct VisibleItemIndex(pub usize);

/// N-th item, may currently be invisible, becomes invalid after any add/remove/move operation
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
pub struct ItemIndex(pub usize);

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct TargetPosition {
    /// before which index to insert, may be in a range of 0..=tree.len() to allow for appending
    pub before: usize,
    /// at which level to insert, if None the level is derived from the item before
    pub level: u8, // TODO go back to Option and implement
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

#[must_use = "iterators are lazy and do nothing unless consumed"]
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

#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct VisibleItemIteratorExtraInfo<'a> {
    items: &'a Vec<Node>,
    /// Index of the next element to return, not guaranteed to be in-bounds
    next_idx: usize,
}

impl<'a> Iterator for VisibleItemIteratorExtraInfo<'a> {
    type Item = (&'a Node, ItemIndex, bool, bool);

    fn next(&mut self) -> Option<Self::Item> {
        let this_idx = self.next_idx;
        if this_idx < self.items.len() {
            self.next_idx = next_visible_item(self.items, this_idx);

            let this_level = self.items[this_idx].level;
            let has_child = self
                .items
                .get(this_idx + 1)
                .map(|item| item.level > this_level)
                .unwrap_or(false);
            Some((
                &self.items[this_idx],
                ItemIndex(this_idx),
                has_child,
                self.next_idx >= self.items.len(),
            ))
        } else {
            None
        }
    }
}

// TODO ancestor path iterator

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
    pub items: Vec<Node>, // TODO make private?
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
        }
    }

    pub fn iter_selected(&self) -> impl Iterator<Item = &Node> + use<'_> {
        self.iter().filter(|i| i.selected)
    }

    pub fn iter_visible_selected(&self) -> impl Iterator<Item = &Node> + use<'_> {
        self.iter_visible().filter(|i| i.selected)
    }

    /// Iterate through items, skipping invisible items, return index of n-th visible item
    pub fn get_visible(&self, index: VisibleItemIndex) -> Option<&Node> {
        self.iter_visible().nth(index.0)
    }

    pub fn get_visible_extra(
        &self,
        index: VisibleItemIndex,
    ) -> Option<(&Node, ItemIndex, bool, bool)> {
        self.iter_visible_extra().nth(index.0)
    }

    pub fn get(&self, index: ItemIndex) -> Option<&Node> {
        self.items.get(index.0)
    }

    pub fn get_mut(&mut self, index: ItemIndex) -> Option<&mut Node> {
        self.items.get_mut(index.0)
    }

    pub fn to_displayed(&self, index: VisibleItemIndex) -> Option<ItemIndex> {
        // TODO make this more efficient
        let item = self.get_visible(index)?;
        self.items
            .iter()
            .position(|x| std::ptr::eq(x, item))
            .map(ItemIndex)
        //let base_ptr = vec.as_ptr();
        //let element_ptr = element as *const T;
        //if element_ptr >= base_ptr && element_ptr < unsafe { base_ptr.add(vec.len()) } {
        //    Some(unsafe { element_ptr.offset_from(base_ptr) as usize })
        //} else {
        //    None
        //}
    }

    /// insert item after offset visible items (either in root or in unfolded parent)
    pub fn insert_item(
        &mut self,
        item: DisplayedItemRef,
        position: TargetPosition,
    ) -> Result<ItemIndex, MoveResult> {
        self.is_valid_position(position)?;

        self.items.insert(
            position.before,
            Node {
                item,
                level: position.level,
                unfolded: true,
                selected: false,
            },
        );

        Ok(ItemIndex(position.before))
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
        self.items.drain(item..end).map(|x| x.item).collect_vec()
    }

    pub fn remove_dissolve(&mut self, ItemIndex(item): ItemIndex) -> DisplayedItemRef {
        let end = self.subtree_end(item);
        self.items[item + 1..end]
            .iter_mut()
            .for_each(|x| x.level -= 1);
        self.items.remove(item).item
    }

    pub fn extract_recursive_if<F>(&mut self, f: F) -> Vec<DisplayedItemRef>
    where
        F: Fn(&Node) -> bool,
    {
        let mut removed = vec![];

        let mut idx = 0;
        while idx < self.items.len() {
            if f(self.items.get(idx).unwrap()) {
                let end = self.subtree_end(idx);
                removed.extend(self.items.drain(idx..end).map(|x| x.item));
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
    ) -> Result<VisibleItemIndex, MoveResult>
    where
        F: Fn(&Node) -> bool,
    {
        let Some(ItemIndex(idx)) = self.to_displayed(vidx) else {
            return Err(MoveResult::InvalidIndex);
        };

        let this_level = self.items[idx].level;
        let end = self.subtree_end(idx);
        let new_index = match direction {
            MoveDir::Down => match self.items.get(end) {
                // we are at the end, but maybe still down in the hierarchy -> shift out
                None => {
                    shift_subtree_to_level(&mut self.items[idx..end], this_level.saturating_sub(1));
                    vidx
                }
                // the next node is less indented -> shift out, don't move yet
                Some(Node { level, .. }) if *level < this_level => {
                    shift_subtree_to_level(&mut self.items[idx..end], this_level - 1);
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
                            before: end + 1,
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
                            before: self.subtree_end(end),
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
                    // .. or node deeper in, but don't move into
                    Some((_node_idx, node))
                        if (node.level >= this_level && f(node) && node.unfolded)
                            | (node.level > this_level) =>
                    {
                        shift_subtree_to_level(&mut self.items[idx..end], this_level + 1);
                        vidx
                    }
                    Some((node_idx, node)) => {
                        self.move_items(
                            vec![ItemIndex(idx)],
                            TargetPosition {
                                before: node_idx,
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
    ///
    /// Deals with any combination of items to move, except the error
    /// cases below. Rules that are followed:
    /// - When calculating the "next" node, visibility is taken into account
    /// - The relative order of element should be the same, before and after the move
    /// - If the root of a subtree is moved, the whole subtree is moved
    /// - If a node inside a subtree is moved, then it's moved out of that subtree
    /// - If both the root and a node of a subtree are moved, the node is moved out
    ///   of the subtree and ends up after the root node
    /// - When moving an element to "After" the last element of a subtree, it goes into the subtree
    /// - When moving an element to "Before" the first element of a subtree (not the root) it also
    ///   goes into the subtree
    /// - When moving to "After" a subtree root, we move into the subtree
    ///
    /// Possible errors:
    /// - trying to move an element into itself -> errors out
    /// - move nests too deep for our level count -> clips items to level 255
    /// - invalid indices -> errors out
    pub fn move_items(
        &mut self,
        mut indices: Vec<ItemIndex>,
        target: TargetPosition,
    ) -> Result<(), MoveResult> {
        indices.sort();
        let indices = indices.into_iter().dedup().collect_vec();

        self.is_valid_position(target)?;
        if let Some(idx) = indices.last() {
            if idx.0 >= self.items.len() {
                return Err(MoveResult::InvalidIndex);
            }
        }

        let pre_split = indices
            .iter()
            .position(|x| x.0 >= target.before)
            .unwrap_or(indices.len());
        let post_split = indices[pre_split..]
            .iter()
            .position(|x| x.0 > target.before)
            .unwrap_or(0);
        let (pre_indices, rem) = indices.split_at(pre_split);
        let (stable, post_indices) = rem.split_at(post_split);
        if stable.len() > 1 {
            panic!("multiple stable elements - this should never happen")
        }

        if self.path_to_root_would_intersect(pre_indices, target.before, target.level) {
            return Err(MoveResult::CircularMove);
        }

        // move all items that are before the target index
        // - do so in reverse -> if a subtree and a node from inside this subtree is moved we move
        //   that subnode correctly and don't need to adjust indices
        // - we need to adjust the insertion point since we go in reverse
        //
        // Note: due to intersect check above we can't have a subtree that crosses over the target_idx
        let mut pre_index_insert = target
            .before
            .min(stable.first().map(|x| x.0).unwrap_or(usize::MAX));
        for &ItemIndex(from_start) in pre_indices.iter().rev() {
            let from_end = self.subtree_end(from_start);

            shift_subtree_to_level(&mut self.items[from_start..from_end], target.level);

            let cnt = from_end - from_start;
            self.items[from_start..pre_index_insert].rotate_left(cnt);
            pre_index_insert -= cnt;
        }

        // correct level of stable subtree
        if !stable.is_empty() {
            let stable_end = self.subtree_end(stable[0].0);
            shift_subtree_to_level(&mut self.items[stable[0].0..stable_end], target.level);
        }

        // move all items that are after the target index
        // - again go in reverse to correctly handle nested items to be moved
        // - all indices need to be adjusted for the number of nodes that we moved in front
        //   of them
        let mut idx_offset = 0;
        let post_index_insert = target
            .before
            .max(stable.first().map(|x| x.0 + 1).unwrap_or(0));
        for &ItemIndex(orig_start) in post_indices.iter().rev() {
            let from_start = orig_start + idx_offset;
            let from_end = self.subtree_end(from_start);

            shift_subtree_to_level(&mut self.items[from_start..from_end], target.level);

            let cnt = from_end - from_start;
            self.items[post_index_insert..from_end].rotate_right(cnt);
            idx_offset += cnt;
        }
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

    /// Checks if the position is valid for the current tree
    ///
    /// Does not do any application logic checks, only whether the position is
    /// in general valid, ignoring visibility and assuming that every node
    /// may have children.
    /// It returns an appropriate error in case the position is invalid.
    fn is_valid_position(&self, _position: TargetPosition) -> Result<(), MoveResult> {
        Ok(())
        // TODO
        /*if position.before > self.items.len() {
            return Err(MoveResult::InvalidIndex);
        }
        let Some(split) = position.before.checked_sub(1) else {
            return match position.level {
                0 => return Ok(()),
                _ => Err(MoveResult::InvalidIndex),
            };
        };

        let valid_range = match self.items[split..split + 2].len() {
            0 => panic!("inconsistent state, length was checked above"),
            1 => 0..self.items[split].level.saturating_add(1),
            _ => self.items[split + 1].level..self.items[split].level.saturating_add(1),
        };

        if valid_range.contains(&position.level) {
            Ok(())
        } else {
            Err(MoveResult::InvalidLevel)
        }*/
    }

    pub fn is_visible(&self, ItemIndex(index): ItemIndex) -> bool {
        self.path_to_root(index, self.items[index].level)
            .iter()
            .all(|x| self.items[*x].unfolded)
    }

    /// Check whether the path from the imaginary node `idx/level` to the root would
    /// intersect with any of the indices in the list.
    ///
    /// Precondition: `indices` must be sorted in ascending order
    fn path_to_root_would_intersect(&self, indices: &[ItemIndex], idx: usize, level: u8) -> bool {
        let mut would_be_parents = self.path_to_root(idx, level);
        would_be_parents.reverse();

        let mut i = 0;
        let mut j = 0;
        while i < indices.len() && j < would_be_parents.len() {
            match indices[i].0.cmp(&would_be_parents[j]) {
                std::cmp::Ordering::Equal => return true,
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
            }
        }

        false
    }

    /// Indices of parents, assuming that `index` and `level` are a valid node
    fn path_to_root(&self, index: usize, mut level: u8) -> Vec<usize> {
        let mut result = vec![];
        for (idx, x) in self.items[..index].iter().enumerate().rev() {
            if x.level < level {
                result.push(idx);
                level = x.level;
            }
            if level == 0 {
                break;
            }
        }
        result
    }

    pub fn xfold(&mut self, ItemIndex(item): ItemIndex, unfolded: bool) {
        self.items[item].unfolded = unfolded;
        if !unfolded {
            self.xselect_subtree(ItemIndex(item), false);
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

    pub fn xfold_subtree(&mut self, ItemIndex(item): ItemIndex, unfolded: bool) {
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

    pub fn xselect_subtree(&mut self, ItemIndex(item): ItemIndex, selected: bool) {
        let end = self.subtree_end(item);
        for x in &mut self.items[item..end] {
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

    pub fn for_each_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut Node),
    {
        for x in &mut self.items {
            f(x);
        }
    }

    pub fn for_each_subtree_mut<F>(&mut self, ItemIndex(item): ItemIndex, mut f: F)
    where
        F: FnMut(&mut Node),
    {
        let end = self.subtree_end(item);
        for x in &mut self.items[item..end] {
            f(x);
        }
    }

    pub fn retain_recursive(&mut self, mut f: impl FnMut(&Node) -> bool) {
        self.items.retain(|x| f(x));
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

fn shift_subtree_to_level(nodes: &mut [Node], target_level: u8) {
    let Some(from_level) = nodes.first().map(|node| node.level) else {
        return;
    };
    let level_corr = (target_level as i16) - (from_level as i16);
    for elem in nodes.iter_mut() {
        elem.level = TryInto::<u8>::try_into(elem.level as i16 + level_corr).unwrap_or(255);
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;

    use super::*;

    fn build_tree(nodes: &[(usize, u8, bool, bool)]) -> DisplayedItemTree {
        let mut tree = DisplayedItemTree::new();
        for &(item, level, unfolded, selected) in nodes {
            tree.items.push(Node {
                item: DisplayedItemRef(item),
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
            tree.iter_visible().map(|x| x.item.0).collect_vec(),
            vec![0, 1, 2, 3, 30, 31, 4, 5]
        );
    }

    #[test]
    fn test_iter_visible_extra() {
        let tree = test_tree();
        assert_eq!(
            tree.iter_visible_extra()
                .map(|(x, idx, child, last)| (x.item.0, idx.0, child, last))
                .collect_vec(),
            vec![
                (0, 0, false, false),
                (1, 1, false, false),
                (2, 2, true, false),
                (3, 5, true, false),
                (30, 6, false, false),
                (31, 7, false, false),
                (4, 8, false, false),
                (5, 9, false, true),
            ]
        )
    }

    #[test]
    fn test_insert_item_before_first() {
        let mut tree = test_tree();
        tree.insert_item(
            DisplayedItemRef(0xff),
            TargetPosition {
                before: 0,
                level: 0,
            },
        )
        .expect("insert_item must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
                before: 8,
                level: 1,
            },
        )
        .expect("insert_item must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
                before: 7,
                level: 2,
            },
        )
        .expect("insert_item must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
                before: 10,
                level: 0,
            },
        )
        .expect("insert_item must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![0, 1, 3, 30, 31, 4, 5]
        );
    }

    #[test]
    fn test_remove_dissolve_with_children() {
        let mut tree = test_tree();
        let removed = tree.remove_dissolve(ItemIndex(5));
        assert_eq!(removed, DisplayedItemRef(3));
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
            .move_item(VisibleItemIndex(3), MoveDir::Up, |node| node.item.0 == 1)
            .expect("move must succeed");
        assert_eq!(new_idx.0, 3);
        assert_eq!(tree.items[3].level, 1);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![0, 1, 10, 2, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Up, |node| node.item.0 == 1)
            .expect("move must succeed");
        assert_eq!(new_idx.0, 2);
        assert_eq!(tree.items[2].level, 1);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![0, 1, 2, 10, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Up, |node| node.item.0 == 1)
            .expect("move must succeed");
        assert_eq!(new_idx.0, 1);
        assert_eq!(tree.items[1].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![0, 2, 1, 10, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Up, |node| node.item.0 == 1)
            .expect("move must succeed");
        assert_eq!(new_idx.0, 0);
        assert_eq!(tree.items[0].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![2, 0, 1, 10, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Up, |node| node.item.0 == 1)
            .expect("move must succeed");
        assert_eq!(new_idx.0, 0);
        assert_eq!(tree.items[0].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
            .move_item(VisibleItemIndex(2), MoveDir::Up, |node| node.item.0 == 1)
            .expect("move must succeed");
        assert_eq!(new_idx.0, 1);
        assert_eq!(tree.items[1].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![0, 2, 1, 10, 11, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Up, |node| node.item.0 == 1)
            .expect("move must succeed");
        assert_eq!(new_idx.0, 0);
        assert_eq!(tree.items[0].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
            .move_item(VisibleItemIndex(1), MoveDir::Down, |node| node.item.0 == 2)
            .expect("move must succeed");
        println!("{:?}", tree.items);
        assert_eq!(new_idx.0, 2);
        assert_eq!(tree.items[3].level, 1);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![0, 2, 1, 20, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Down, |node| node.item.0 == 2)
            .expect("move must succeed");
        println!("{:?}", tree.items);
        assert_eq!(new_idx.0, 3);
        assert_eq!(tree.items[3].level, 1);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![0, 2, 20, 1, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Down, |node| node.item.0 == 2)
            .expect("move must succeed");
        println!("{:?}", tree.items);
        assert_eq!(new_idx.0, 3);
        assert_eq!(tree.items[3].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![0, 2, 20, 1, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Down, |node| node.item.0 == 2)
            .expect("move must succeed");
        println!("{:?}", tree.items);
        assert_eq!(new_idx.0, 4);
        assert_eq!(tree.items[3].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![0, 2, 20, 3, 1]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Down, |node| node.item.0 == 2)
            .expect("move must succeed");
        println!("{:?}", tree.items);
        assert_eq!(new_idx.0, 4);
        assert_eq!(tree.items[3].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![0, 2, 20, 3, 1]
        );
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
        let new_idx = tree
            .move_item(VisibleItemIndex(1), MoveDir::Down, |node| node.item.0 == 2)
            .expect("move must succeed");
        println!("{:?}", tree.items);
        assert_eq!(new_idx.0, 2);
        assert_eq!(tree.items[3].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![0, 2, 20, 1, 3]
        );

        let new_idx = tree
            .move_item(new_idx, MoveDir::Down, |node| node.item.0 == 2)
            .expect("move must succeed");
        println!("{:?}", tree.items);
        assert_eq!(new_idx.0, 3);
        assert_eq!(tree.items[3].level, 0);
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![0, 2, 20, 3, 1]
        );
    }

    #[test]
    fn test_move_items_single_to_start() {
        let mut tree = test_tree();
        tree.move_items(
            vec![ItemIndex(8)],
            TargetPosition {
                before: 0,
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
                before: 10,
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
                before: 1,
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
                before: 1,
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
                before: 5,
                level: 1,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
                before: 4,
                level: 2,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
                before: 3,
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
                before: 1,
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
                before: 2,
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(tree.items, ref_tree.items);
    }
    #[test]
    fn test_move_items_inbetween_selected_same_depth() {
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
                before: 2,
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
                before: 4,
                level: 2,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
                before: 4,
                level: 1,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
                before: 4,
                level: 0,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
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
                before: 4,
                level: 2,
            },
        )
        .expect("move_items must succeed");
        assert_eq!(
            tree.items.iter().map(|x| x.item.0).collect_vec(),
            vec![0, 10, 12, 11, 13]
        );
    }

    #[test]
    /// Moving "after" a node that has children moves into the subtree,
    /// so we must error out if the node itself should be moved
    fn test_move_items_reject_after_self_into_subtree() {
        let mut tree = test_tree();
        let result = tree.move_items(
            vec![ItemIndex(0), ItemIndex(3), ItemIndex(9)],
            TargetPosition {
                before: 4,
                level: 2,
            },
        );
        assert_eq!(result, Err(MoveResult::CircularMove));
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
                before: 4,
                level: 2,
            },
        );
        assert_eq!(result, Err(MoveResult::CircularMove));
        assert_eq!(tree.items, reference.items);
    }

    #[test]
    fn test_move_items_reject_into_self() {
        let mut tree = test_tree();
        let result = tree.move_items(
            vec![ItemIndex(0)],
            TargetPosition {
                before: 1,
                level: 1,
            },
        );
        assert_eq!(result, Err(MoveResult::CircularMove));
        assert_eq!(tree.items, test_tree().items);
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
