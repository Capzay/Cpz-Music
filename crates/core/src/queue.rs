//! What plays next. Kept apart from the audio engine because this is the only part
//! with real logic in it, and off-by-one errors here show up as "shuffle repeats a
//! song" rather than as a crash.

use serde::{Deserialize, Serialize};

use crate::types::{PlayerTrack, RepeatMode};

/// What the player has to do after the queue moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The current track changed: load it and play from the start.
    Changed,
    /// Same track, from the top.
    Restart,
    /// Nothing left to play.
    Stop,
}

/// The queue as the remotes see it. Shuffle bookkeeping stays on the device that
/// is actually playing, so it is not part of this.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QueueView {
    pub tracks: Vec<PlayerTrack>,
    pub index: usize,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

#[derive(Debug)]
pub struct Queue {
    tracks: Vec<PlayerTrack>,
    index: usize,
    shuffle: bool,
    /// A permutation of queue indices, starting with whatever was playing when
    /// shuffle was turned on.
    order: Vec<usize>,
    /// Where in `order` the current track is.
    pos: usize,
    repeat: RepeatMode,
    rng: fastrand::Rng,
}

impl Default for Queue {
    fn default() -> Self {
        Self::with_rng(fastrand::Rng::new())
    }
}

/// A permutation that starts at the track playing now, so turning shuffle on
/// never interrupts the current song.
fn shuffle_order(len: usize, current: usize, rng: &mut fastrand::Rng) -> Vec<usize> {
    let mut rest: Vec<usize> = (0..len).filter(|&i| i != current).collect();
    rng.shuffle(&mut rest);
    let mut order = Vec::with_capacity(len);
    if current < len {
        order.push(current);
    }
    order.extend(rest);
    order
}

impl Queue {
    pub fn with_rng(rng: fastrand::Rng) -> Self {
        Self {
            tracks: Vec::new(),
            index: 0,
            shuffle: false,
            order: Vec::new(),
            pos: 0,
            repeat: RepeatMode::Off,
            rng,
        }
    }

    pub fn tracks(&self) -> &[PlayerTrack] {
        &self.tracks
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    /// Always whatever the index points at, never a copy that can drift.
    pub fn current(&self) -> Option<&PlayerTrack> {
        self.tracks.get(self.index)
    }

    pub fn view(&self) -> QueueView {
        QueueView {
            tracks: self.tracks.clone(),
            index: self.index,
            shuffle: self.shuffle,
            repeat: self.repeat,
        }
    }

    /// Takes on what the active device reported. Shuffle order is rebuilt locally
    /// so this device can carry on if it becomes the active one.
    pub fn adopt(&mut self, view: QueueView) {
        self.index = view.index.min(view.tracks.len().saturating_sub(1));
        self.tracks = view.tracks;
        self.repeat = view.repeat;
        self.shuffle = view.shuffle;
        self.reshuffle_from(self.index);
    }

    fn reshuffle_from(&mut self, current: usize) {
        self.pos = 0;
        self.order = if self.shuffle {
            shuffle_order(self.tracks.len(), current, &mut self.rng)
        } else {
            Vec::new()
        };
    }

    /// Replaces the queue. Returns whether there is anything to play.
    pub fn set(&mut self, tracks: Vec<PlayerTrack>, start: usize) -> bool {
        self.index = start.min(tracks.len().saturating_sub(1));
        self.tracks = tracks;
        self.reshuffle_from(self.index);
        !self.tracks.is_empty()
    }

    /// Plays a track: jumps to it if it is queued already, otherwise it becomes
    /// the whole queue.
    pub fn play(&mut self, track: PlayerTrack) {
        match self.tracks.iter().position(|queued| queued.id == track.id) {
            Some(at) => {
                self.jump_to(at);
            }
            None => {
                self.set(vec![track], 0);
            }
        }
    }

    pub fn advance(&mut self) -> Step {
        if self.tracks.is_empty() {
            return Step::Stop;
        }
        // Repeat-one wins over everything, including the end of the queue.
        if self.repeat == RepeatMode::One {
            return Step::Restart;
        }

        if self.shuffle && !self.order.is_empty() {
            if self.pos + 1 < self.order.len() {
                self.pos += 1;
                self.index = self.order[self.pos];
                return Step::Changed;
            }
            if self.repeat != RepeatMode::All {
                return Step::Stop;
            }
            // Ran out of shuffled tracks: a fresh order, carrying on from where the
            // last one started so the length of the cycle stays the same.
            let first = self.order[0];
            self.order = shuffle_order(self.tracks.len(), first, &mut self.rng);
            self.pos = 0;
            self.index = self.order[0];
            return Step::Changed;
        }

        if self.index + 1 < self.tracks.len() {
            self.index += 1;
            Step::Changed
        } else if self.repeat == RepeatMode::All {
            self.index = 0;
            Step::Changed
        } else {
            Step::Stop
        }
    }

    /// Restarts the current track once more than three seconds have played,
    /// matching what every other music player does with a back button.
    pub fn back(&mut self, current_time: f64) -> Step {
        if self.tracks.is_empty() {
            return Step::Stop;
        }
        if current_time > 3.0 {
            return Step::Restart;
        }
        if self.shuffle && !self.order.is_empty() {
            self.pos = self.pos.saturating_sub(1);
            self.index = self.order[self.pos];
        } else {
            self.index = self.index.saturating_sub(1);
        }
        Step::Changed
    }

    /// Queues a track straight after the current one, in shuffle order too.
    pub fn play_next(&mut self, track: PlayerTrack) {
        let at = (self.index + 1).min(self.tracks.len());
        self.tracks.insert(at, track);
        if self.shuffle && !self.order.is_empty() {
            for slot in &mut self.order {
                if *slot >= at {
                    *slot += 1;
                }
            }
            self.order.insert(self.pos + 1, at);
        }
    }

    pub fn append(&mut self, track: PlayerTrack) {
        self.tracks.push(track);
        if self.shuffle && !self.order.is_empty() {
            self.order.push(self.tracks.len() - 1);
        }
    }

    /// Refuses to remove what is playing, which would leave the audio engine on a
    /// track no longer in the queue.
    pub fn remove(&mut self, at: usize) -> bool {
        if at == self.index || at >= self.tracks.len() {
            return false;
        }
        self.tracks.remove(at);
        if at < self.index {
            self.index -= 1;
        }
        if self.shuffle && !self.order.is_empty() {
            self.order.retain(|&slot| slot != at);
            for slot in &mut self.order {
                if *slot > at {
                    *slot -= 1;
                }
            }
            self.pos = self
                .order
                .iter()
                .position(|&slot| slot == self.index)
                .unwrap_or(0);
        }
        true
    }

    pub fn jump_to(&mut self, at: usize) -> bool {
        if at >= self.tracks.len() {
            return false;
        }
        self.index = at;
        // Keep the shuffle cursor on the track actually playing, or the next track
        // would be picked relative to the wrong position.
        if self.shuffle {
            self.pos = self.order.iter().position(|&slot| slot == at).unwrap_or(0);
        }
        true
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.reshuffle_from(self.index);
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.next();
    }
}

/// Moves one item within a list, clamping the destination to the end.
pub fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) {
    if from == to || from >= items.len() {
        return;
    }
    let to = to.min(items.len() - 1);
    let item = items.remove(from);
    items.insert(to, item);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{TrackAlbum, TrackArtist};

    fn track(id: i32) -> PlayerTrack {
        PlayerTrack {
            id,
            title: format!("Track {id}"),
            duration: Some(180.0),
            track_number: Some(id),
            disc_number: 1,
            artist: TrackArtist {
                id: 1,
                name: "Artist".into(),
            },
            album: TrackAlbum {
                id: 1,
                title: "Album".into(),
                has_artwork: false,
            },
        }
    }

    fn queue(len: i32, start: usize) -> Queue {
        let mut queue = Queue::with_rng(fastrand::Rng::with_seed(7));
        queue.set((0..len).map(track).collect(), start);
        queue
    }

    fn sorted(order: &[usize]) -> Vec<usize> {
        let mut order = order.to_vec();
        order.sort_unstable();
        order
    }

    #[test]
    fn shuffle_keeps_the_current_track_first_and_loses_nothing() {
        let mut rng = fastrand::Rng::with_seed(1);
        let order = shuffle_order(50, 10, &mut rng);
        assert_eq!(order[0], 10);
        assert_eq!(sorted(&order), (0..50).collect::<Vec<_>>());
        assert_ne!(order, (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn shuffle_handles_empty_and_out_of_range_queues() {
        let mut rng = fastrand::Rng::with_seed(1);
        assert!(shuffle_order(0, 0, &mut rng).is_empty());
        assert_eq!(sorted(&shuffle_order(3, 99, &mut rng)), vec![0, 1, 2]);
    }

    #[test]
    fn advances_then_stops_at_the_end_with_repeat_off() {
        let mut q = queue(5, 3);
        assert_eq!(q.advance(), Step::Changed);
        assert_eq!(q.index(), 4);
        assert_eq!(q.advance(), Step::Stop);
        assert_eq!(q.index(), 4);
    }

    #[test]
    fn wraps_with_repeat_all_and_restarts_with_repeat_one() {
        let mut q = queue(5, 4);
        q.cycle_repeat();
        assert_eq!(q.advance(), Step::Changed);
        assert_eq!(q.index(), 0);

        q.cycle_repeat();
        assert_eq!(q.repeat(), RepeatMode::One);
        assert_eq!(q.advance(), Step::Restart);
        assert_eq!(q.index(), 0);
    }

    #[test]
    fn follows_the_shuffle_order_and_plays_every_track_once() {
        let mut q = queue(6, 2);
        q.toggle_shuffle();
        let mut played = vec![q.index()];
        while q.advance() == Step::Changed {
            played.push(q.index());
        }
        assert_eq!(played[0], 2);
        assert_eq!(sorted(&played), (0..6).collect::<Vec<_>>());
    }

    #[test]
    fn reshuffles_after_the_last_shuffled_track_with_repeat_all() {
        let mut q = queue(4, 0);
        q.toggle_shuffle();
        q.cycle_repeat();
        for _ in 0..3 {
            assert_eq!(q.advance(), Step::Changed);
        }
        assert_eq!(q.advance(), Step::Changed);
        assert_eq!(q.pos, 0);
        assert_eq!(q.index(), q.order[0]);
        assert_eq!(sorted(&q.order), vec![0, 1, 2, 3]);
    }

    #[test]
    fn an_empty_queue_stops() {
        let mut q = Queue::default();
        assert_eq!(q.advance(), Step::Stop);
        assert_eq!(q.back(0.0), Step::Stop);
        assert!(!q.set(Vec::new(), 0));
    }

    #[test]
    fn back_restarts_past_three_seconds_and_steps_back_before() {
        let mut q = queue(5, 2);
        assert_eq!(q.back(10.0), Step::Restart);
        assert_eq!(q.index(), 2);
        assert_eq!(q.back(1.0), Step::Changed);
        assert_eq!(q.index(), 1);
        q.jump_to(0);
        assert_eq!(q.back(0.0), Step::Changed);
        assert_eq!(q.index(), 0);
    }

    #[test]
    fn back_walks_the_shuffle_order() {
        let mut q = queue(5, 0);
        q.toggle_shuffle();
        q.advance();
        q.advance();
        let expected = q.order[1];
        q.back(0.0);
        assert_eq!(q.index(), expected);
        assert_eq!(q.pos, 1);
    }

    #[test]
    fn play_next_goes_straight_after_the_current_track() {
        let mut q = queue(5, 1);
        q.play_next(track(99));
        assert_eq!(q.tracks()[2].id, 99);
        q.advance();
        assert_eq!(q.current().unwrap().id, 99);
    }

    #[test]
    fn play_next_while_shuffled_is_next_in_the_shuffle_order() {
        let mut q = queue(5, 1);
        q.toggle_shuffle();
        q.play_next(track(99));
        assert_eq!(sorted(&q.order), (0..6).collect::<Vec<_>>());
        q.advance();
        assert_eq!(q.current().unwrap().id, 99);
    }

    #[test]
    fn appended_tracks_join_the_shuffle_order() {
        let mut q = queue(3, 0);
        q.toggle_shuffle();
        q.append(track(99));
        assert_eq!(sorted(&q.order), vec![0, 1, 2, 3]);
    }

    #[test]
    fn removing_keeps_the_current_track_playing() {
        let mut q = queue(5, 3);
        assert!(!q.remove(3));
        assert!(q.remove(1));
        assert_eq!(q.index(), 2);
        assert_eq!(q.current().unwrap().id, 3);
        assert!(!q.remove(99));
    }

    #[test]
    fn removing_while_shuffled_keeps_a_valid_order_and_cursor() {
        let mut q = queue(10, 4);
        q.toggle_shuffle();
        q.advance();
        let playing = q.current().unwrap().id;
        let victim = (0..10).find(|&i| i != q.index()).unwrap();
        assert!(q.remove(victim));
        assert_eq!(sorted(&q.order), (0..9).collect::<Vec<_>>());
        assert_eq!(q.order[q.pos], q.index());
        assert_eq!(q.current().unwrap().id, playing);
    }

    #[test]
    fn jumping_moves_the_shuffle_cursor_to_match() {
        let mut q = queue(6, 0);
        q.toggle_shuffle();
        let target = q.order[4];
        assert!(q.jump_to(target));
        assert_eq!(q.pos, 4);
        assert!(!q.jump_to(6));
    }

    #[test]
    fn playing_a_queued_track_jumps_to_it_and_a_new_one_replaces_the_queue() {
        let mut q = queue(5, 0);
        q.play(track(3));
        assert_eq!(q.index(), 3);
        assert_eq!(q.tracks().len(), 5);
        q.play(track(42));
        assert_eq!(q.tracks().len(), 1);
        assert_eq!(q.current().unwrap().id, 42);
    }

    #[test]
    fn adopting_a_remote_view_clamps_the_index() {
        let mut q = Queue::default();
        q.adopt(QueueView {
            tracks: vec![track(1), track(2)],
            index: 9,
            shuffle: true,
            repeat: RepeatMode::All,
        });
        assert_eq!(q.index(), 1);
        assert_eq!(q.order[0], 1);
    }

    #[test]
    fn move_item_clamps_and_ignores_bad_sources() {
        let mut items = vec![1, 2, 3, 4];
        move_item(&mut items, 0, 2);
        assert_eq!(items, vec![2, 3, 1, 4]);
        move_item(&mut items, 3, 99);
        assert_eq!(items, vec![2, 3, 1, 4]);
        move_item(&mut items, 0, 99);
        assert_eq!(items, vec![3, 1, 4, 2]);
        move_item(&mut items, 9, 0);
        assert_eq!(items, vec![3, 1, 4, 2]);
    }
}
