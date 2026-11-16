//! 목록의 다중 선택 상태 (Windows 탐색기와 같은 클릭 / Ctrl+클릭 / Shift+클릭 규칙).

use std::collections::HashSet;
use std::hash::Hash;

#[derive(Clone, Debug)]
pub struct Selection<K> {
    items: HashSet<K>,
    /// Shift+클릭 범위 선택의 기준점 (마지막으로 일반/Ctrl 클릭한 항목).
    anchor: Option<K>,
}

impl<K> Default for Selection<K> {
    fn default() -> Self {
        Self {
            items: HashSet::new(),
            anchor: None,
        }
    }
}

impl<K: Eq + Hash + Clone> Selection<K> {
    pub fn contains(&self, key: &K) -> bool {
        self.items.contains(key)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.anchor = None;
    }

    /// 이 항목 하나만 선택한다.
    pub fn select_only(&mut self, key: K) {
        self.items.clear();
        self.items.insert(key.clone());
        self.anchor = Some(key);
    }

    pub fn select_all(&mut self, order: &[K]) {
        self.items = order.iter().cloned().collect();
        if self.anchor.is_none() {
            self.anchor = order.first().cloned();
        }
    }

    /// 왼쪽 클릭. `order`는 화면에 보이는 순서의 전체 키 목록이다.
    ///
    /// - 일반 클릭: 그 항목만 선택
    /// - Ctrl+클릭: 그 항목의 선택을 뒤집음
    /// - Shift+클릭: 기준점부터 그 항목까지 범위 선택 (기존 선택 대체)
    /// - Ctrl+Shift+클릭: 범위를 기존 선택에 추가
    pub fn click(&mut self, key: &K, order: &[K], ctrl: bool, shift: bool) {
        if shift && let Some(range) = self.range_to(key, order) {
            if !ctrl {
                self.items.clear();
            }
            self.items.extend(range.iter().cloned());
            return;
        }
        if ctrl {
            if !self.items.remove(key) {
                self.items.insert(key.clone());
            }
            self.anchor = Some(key.clone());
        } else {
            self.select_only(key.clone());
        }
    }

    /// 오른쪽 클릭. 이미 선택된 항목이면 선택을 유지하고, 아니면 그 항목만 선택한다.
    pub fn context_click(&mut self, key: &K) {
        if !self.contains(key) {
            self.select_only(key.clone());
        }
    }

    /// 더 이상 존재하지 않거나 보이지 않는 항목을 선택에서 뺀다.
    pub fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) {
        self.items.retain(|k| keep(k));
        if self.anchor.as_ref().is_some_and(|a| !keep(a)) {
            self.anchor = None;
        }
    }

    fn range_to<'a>(&self, key: &K, order: &'a [K]) -> Option<&'a [K]> {
        let anchor = self.anchor.as_ref()?;
        let a = order.iter().position(|k| k == anchor)?;
        let b = order.iter().position(|k| k == key)?;
        Some(&order[a.min(b)..=a.max(b)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDER: [u32; 6] = [10, 20, 30, 40, 50, 60];

    fn sorted(sel: &Selection<u32>) -> Vec<u32> {
        let mut v: Vec<u32> = ORDER.iter().copied().filter(|k| sel.contains(k)).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn plain_click_selects_one() {
        let mut sel = Selection::default();
        sel.click(&20, &ORDER, false, false);
        sel.click(&40, &ORDER, false, false);
        assert_eq!(sorted(&sel), [40]);
    }

    #[test]
    fn ctrl_click_toggles() {
        let mut sel = Selection::default();
        sel.click(&20, &ORDER, false, false);
        sel.click(&40, &ORDER, true, false);
        sel.click(&60, &ORDER, true, false);
        assert_eq!(sorted(&sel), [20, 40, 60]);
        sel.click(&40, &ORDER, true, false);
        assert_eq!(sorted(&sel), [20, 60]);
    }

    #[test]
    fn shift_click_selects_range_both_directions() {
        let mut sel = Selection::default();
        sel.click(&30, &ORDER, false, false);
        sel.click(&50, &ORDER, false, true);
        assert_eq!(sorted(&sel), [30, 40, 50]);
        // 기준점은 그대로이므로 반대 방향으로 다시 잡을 수 있다.
        sel.click(&10, &ORDER, false, true);
        assert_eq!(sorted(&sel), [10, 20, 30]);
    }

    #[test]
    fn ctrl_shift_click_adds_range() {
        let mut sel = Selection::default();
        sel.click(&10, &ORDER, false, false);
        sel.click(&40, &ORDER, true, false); // 기준점 40
        sel.click(&60, &ORDER, true, true);
        assert_eq!(sorted(&sel), [10, 40, 50, 60]);
    }

    #[test]
    fn shift_click_without_anchor_acts_like_plain_click() {
        let mut sel = Selection::default();
        sel.click(&30, &ORDER, false, true);
        assert_eq!(sorted(&sel), [30]);
        // 기준점이 목록에서 사라진 경우도 마찬가지.
        sel.retain(|k| *k != 30);
        sel.click(&50, &ORDER, false, true);
        assert_eq!(sorted(&sel), [50]);
    }

    #[test]
    fn context_click_keeps_existing_multi_selection() {
        let mut sel = Selection::default();
        sel.click(&20, &ORDER, false, false);
        sel.click(&40, &ORDER, false, true);
        sel.context_click(&30);
        assert_eq!(sorted(&sel), [20, 30, 40]);
        sel.context_click(&60);
        assert_eq!(sorted(&sel), [60]);
    }

    #[test]
    fn retain_and_select_all() {
        let mut sel = Selection::default();
        sel.select_all(&ORDER);
        assert_eq!(sel.len(), 6);
        sel.retain(|k| *k > 30);
        assert_eq!(sorted(&sel), [40, 50, 60]);
        sel.clear();
        assert!(sel.is_empty());
    }
}
