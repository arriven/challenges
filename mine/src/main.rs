use anyhow::Result;
use core::ops::Add;
use sorted_vec::SortedVec;
use std::{
    collections::VecDeque,
    fs::File,
    io::{BufRead, BufReader},
};
use num::BigInt;

const INPUT: &str = "challenge_input.txt";
const MINE_SIZE: usize = 500;
// should be equal to {MINE_SIZE} in general
// but can be different for benchmarking purposes
const SAFE_SIZE: usize = 500;

fn main() -> Result<()> {
    let file = File::open(INPUT)?;
    let mut mine = Mine::with_capacity(MINE_SIZE);
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line: BigInt = line?.parse()?;
        // check if the number is safe
        // first {SAFE_SIZE} numbers are always safe
        if mine.len() >= SAFE_SIZE && !mine.has_sum(&line) {
            println!("bad input at line {i}: {line}");
        }
        // update the mine
        if mine.len() < MINE_SIZE {
            mine.push(line);
        } else {
            mine.pop().expect("non-empty mine");
            mine.push(line);
        }
    }
    Ok(())
}


/// a helper struct to allow checking for the existing sum in O(n) where n = {MINE_SIZE}
/// instead of O(n^2) with naive approach reducing the overall algorithm complexity from
/// O(m*n^2) to O(m*n) where m is the amount of number mined
pub struct Mine<T>
where
    T: Ord + Add<Output = T> + Clone + std::fmt::Debug + 'static,
    for <'a> &'a T: Add<Output = T>,
{
    // to keep track of which items have to be removed
    // VecDeque uses ring buffer under the hood
    // amortized O(1) becomes true O(1) due to known capacity
    queue: VecDeque<T>,
    // to be able to check the sum in O(n)
    // Note: we could also use BTreeSet but since the target {MINE_SIZE} is small
    // O(n) of insert/remove in vector is faster on modern CPUs than O(log(n)) of
    // BTreeSet due to cache friendliness of vector
    cache: SortedVec<T>,
}

impl<T> Mine<T>
where
    T: Ord + Add<Output = T> + Clone + std::fmt::Debug + 'static,
    for <'a> &'a T: Add<Output = T>,
{
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            cache: SortedVec::new(),
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(cap),
            cache: SortedVec::with_capacity(cap),
        }
    }

    pub fn push(&mut self, item: T) {
        self.queue.push_back(item.clone());
        self.cache.insert(item);
    }

    pub fn pop(&mut self) -> Option<T> {
        if let Some(removed) = self.queue.pop_front() {
            self.cache.remove_item(&removed).expect("item present in cache and queue");
            Some(removed)
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_consistent(&self) -> bool {
        let mut queue: Vec<_> = self.queue.iter().collect();
        let mut cache: Vec<_> = self.cache.iter().collect();
        queue.sort();
        cache.sort();
        queue == cache
    }

    #[cfg(not(feature = "naive"))]
    pub fn has_sum(&self, item: &T) -> bool {
        let mut fwd = 0;
        let mut rev = self.cache.len() - 1;
        while fwd < rev {
            let sum = &self.cache[fwd] + &self.cache[rev];
            match sum.cmp(item) {
                std::cmp::Ordering::Less => fwd += 1,
                std::cmp::Ordering::Greater => rev -= 1,
                std::cmp::Ordering::Equal => 
                {
                    #[cfg(feature = "cmp")]
                    assert_eq!(self.has_sum_naive(item), true);
                    
                    return true
                },
            }
        }

        #[cfg(feature = "cmp")]
        assert_eq!(self.has_sum_naive(item), false, "{item:?}");

        false
    }

    #[cfg(feature = "naive")]
    pub fn has_sum(&self, item: &T) -> bool {
        self.has_sum_naive(item)
    }

    // just for correctness/performance comparison purposes
    #[cfg(any(feature = "naive", feature = "cmp"))]
    fn has_sum_naive(&self, item: &T) -> bool {
        for i in 0..self.queue.len() {
            for j in (i+1)..self.queue.len() {
                if &self.cache[i] + &self.cache[j] == *item {
                    return true;
                }
            }
        }

        false
    }
}
