// git-dit - the distributed issue tracker for git
// Copyright (C) 2025 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//! Object related tests and testing utilities

use super::*;

use std::borrow::Borrow;
use std::hash::{self, Hash};

use crate::base::tests::TestOid;

#[derive(Clone, Debug)]
pub enum TestObject {
    Commit(TestCommit),
    Tree(TestTree),
}

impl Borrow<TestOid> for TestObject {
    fn borrow(&self) -> &TestOid {
        match self {
            Self::Commit(c) => &c.oid,
            Self::Tree(t) => &t.oid,
        }
    }
}

impl Eq for TestObject {}

impl PartialEq for TestObject {
    fn eq(&self, other: &Self) -> bool {
        <TestOid as PartialEq>::eq(self.borrow(), other.borrow())
    }
}

impl Hash for TestObject {
    fn hash<H>(&self, state: &mut H)
    where
        H: hash::Hasher,
    {
        TestOid::hash(self.borrow(), state)
    }
}

#[derive(Clone, Debug)]
pub struct TestCommit {
    oid: TestOid,
    author: String,
    committer: String,
    message: String,
    tree: TestOid,
    parents: Vec<TestOid>,
}

impl commit::Commit for TestCommit {
    type Oid = TestOid;
    type Signature<'s> = &'s str;

    fn id(&self) -> Self::Oid {
        self.oid.clone()
    }

    fn author(&self) -> Self::Signature<'_> {
        self.author.as_ref()
    }

    fn committer(&self) -> Self::Signature<'_> {
        self.committer.as_ref()
    }

    fn message(&self) -> Result<&str, std::str::Utf8Error> {
        Ok(self.message.as_ref())
    }

    fn parent_ids(&self) -> impl IntoIterator<Item = Self::Oid> + '_ {
        self.parents.clone()
    }

    fn tree_id(&self) -> Self::Oid {
        self.tree.clone()
    }
}

#[derive(Clone, Debug)]
pub struct TestTree {
    oid: TestOid,
}
