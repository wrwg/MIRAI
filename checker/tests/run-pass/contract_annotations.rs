// Copyright (c) Facebook, Inc. and its affiliates.
//
// This source code is licensed under the MIT license found in the
// LICENSE file in the root directory of this source tree.
//

// Tests for annotations from the contracts crate.

// TODO: those tests currently do not do anything because all relevant functions are marked
// as angelic. Once this is not longer the case, real assertion logic should be added.

use contracts::*;
use mirai_annotations::*;

pub fn main() {
    use_pre_post();
    use_trait();
    use_invariant();
}

// Simple pre/post
// ---------------

#[pre(x > 0)]
#[post(ret >= x)]
pub fn pre_post(x: i32) -> i32 {
    return x;
}

fn use_pre_post() {
    verify!(pre_post(1) >= 1);
}

// Trait pre/post

#[contract_trait]
trait Adder {
    fn get(&self) -> i32;

    #[pre(x > 0)]
    #[pre(self.get() <= std::i32::MAX - x)]
    #[post(ret == old(self.get()) && self.get() > old(self.get()))]
    fn get_and_add(&mut self, x: i32) -> i32;
}

struct MyAdder {
    x: i32,
}

#[contract_trait]
impl Adder for MyAdder {
    fn get(&self) -> i32 {
        self.x
    }
    fn get_and_add(&mut self, x: i32) -> i32 {
        let c = self.x;
        // The below is currently needed because in MIRAI compilation mode, the runtime
        // assertion generated by the matching #[pre] is dead code. Therefore the compiler
        // does not know that this overflow is protected and issues a warning.
        // TODO: find a way to fix this as it is a general problem
        assert!(self.get() <= std::i32::MAX - x);

        self.x = self.x + x;
        return c;
    }
}

fn use_trait() {
    let mut a = MyAdder { x: 1 };
    verify!(a.get() == 1);
    verify!(a.get_and_add(2) == 1);
    verify!(a.get() == 3);
}

// Invariants
// ==========

struct S {
    x: i32,
}

#[debug_invariant(self.x > 0)]
impl S {
    #[pre(self.x < std::i32::MAX)]
    #[post(ret == old(self.x))]
    fn get_and_decrement(&mut self) -> i32 {
        let c = self.x;
        assert!(self.x < std::i32::MAX); // see above
        self.x = self.x + 1;
        return c;
    }
}

fn use_invariant() {
    let mut s = S { x: 1 };
    verify!(s.get_and_decrement() == 1);
}
