#![allow(unused)]

/// unit tests for cartesian3 and cartographic
/// run with "cargo test --test test_ring_deque test_sortin -- --nocapture"

use std::collections::VecDeque;
use odin_common::collections::RingDeque;

#[derive(Debug)]
struct TestElement(usize);


#[test]
fn test_sortin () {
    println!("--- testing ringbuffer sort_in");
    let mut ring: VecDeque<usize> = RingDeque::new(5);

    let sf = |a:&usize, b: &usize| { a < b };
    for d in [1,10,5,3,7,9,2,8] {
        let res = ring.sort_into_ringbuffer( d, sf);
        print!("{} -> {:?} : [", d, res);
        for e in ring.iter() { print!("{e:?},") };
        println!("]");
    }

    assert_eq!( vec![5,7,8,9,10], ring.to_vec());
}

#[test]
fn test_insert () {
    println!("--- testing ringbuffer insert");
    let mut ring: VecDeque<usize> = RingDeque::new(5);
    for d in 0..9 {
        ring.push_to_ringbuffer(d);
    }
    println!("{ring:?} insert 42 at index 2:");
    ring.insert_into_ringbuffer(2, 42);
    println!("{ring:?}");

    assert_eq!( vec![5,6,42,7,8], ring.to_vec());
}
