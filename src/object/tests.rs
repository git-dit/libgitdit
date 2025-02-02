// git-dit - the distributed issue tracker for git
// Copyright (C) 2025 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//! Object related tests and testing utilities

use super::*;

use std::borrow::Borrow;
use std::collections::HashSet;
use std::hash::{self, Hash};
use std::sync;

use crate::base::tests::TestOid;
use crate::error::tests::TestError;

#[derive(Default, Debug)]
pub struct TestOdb {
    objects: sync::RwLock<HashSet<TestObject>>,
    id_counter: sync::Mutex<TestOid>,
    author: String,
    committer: String,
}

impl TestOdb {
    pub fn with_objects(mut self, objects: impl Iterator<Item = TestObject>) -> Self {
        let db = self.objects.get_mut().expect("Could not access objects");
        db.extend(objects);
        let id = self.id_counter.get_mut().expect("Could not write oid");
        *id = db
            .iter()
            .map(Borrow::<TestOid>::borrow)
            .max()
            .cloned()
            .unwrap_or_default();
        self
    }

    pub fn with_author(self, author: String) -> Self {
        Self { author, ..self }
    }

    pub fn with_committer(self, committer: String) -> Self {
        Self { committer, ..self }
    }

    pub fn ro_objects(&self) -> sync::RwLockReadGuard<'_, HashSet<TestObject>> {
        self.objects.read().expect("Could not read object")
    }

    fn next_oid(&self) -> TestOid {
        let mut oid = self.id_counter.lock().expect("Could not compute next oid");
        *oid += 1;
        oid.clone()
    }
}

impl<'r> Database<'r> for TestOdb {
    type Commit = TestCommit;
    type Tree = TestTree;
    type Signature<'s> = &'s str;
    type TreeBuilder = TestTreeBuilder<'r>;

    fn author(&self) -> error::Result<Self::Signature<'_>, Self::InnerError> {
        Ok(self.author.as_ref())
    }

    fn committer(&self) -> error::Result<Self::Signature<'_>, Self::InnerError> {
        Ok(self.committer.as_ref())
    }

    fn find_commit(&'r self, oid: Self::Oid) -> error::Result<Self::Commit, Self::InnerError> {
        self.ro_objects()
            .get(&oid)
            .and_then(|o| {
                if let TestObject::Commit(c) = o {
                    Some(c.clone())
                } else {
                    None
                }
            })
            .ok_or(TestError)
            .wrap_with_kind(error::Kind::CannotGetCommit)
    }

    fn find_tree(&'r self, oid: Self::Oid) -> error::Result<Self::Tree, Self::InnerError> {
        self.ro_objects()
            .get(&oid)
            .and_then(|o| {
                if let TestObject::Tree(t) = o {
                    Some(t.clone())
                } else {
                    None
                }
            })
            .ok_or(TestError)
            .wrap_with_kind(error::Kind::CannotGetTree)
    }

    fn commit<'s>(
        &'r self,
        author: &Self::Signature<'s>,
        committer: &Self::Signature<'s>,
        message: &str,
        tree: &Self::Tree,
        parents: &[&Self::Commit],
    ) -> error::Result<Self::Oid, Self::InnerError> {
        let oid = self.next_oid();
        let commit = TestCommit {
            oid: oid.clone(),
            author: author.to_string(),
            committer: committer.to_string(),
            message: message.to_owned(),
            tree: tree.oid.clone(),
            parents: parents.iter().map(|c| c.oid.clone()).collect(),
        };
        self.objects
            .write()
            .expect("Could not write object")
            .insert(TestObject::Commit(commit));
        Ok(oid)
    }

    fn empty_tree_builder(&'r self) -> error::Result<Self::TreeBuilder, Self::InnerError> {
        let objects = self.objects.write().expect("Could not write object");
        Ok(TestTreeBuilder {
            objects,
            oid: self.next_oid(),
        })
    }

    fn tree_builder(
        &'r self,
        tree: &Self::Tree,
    ) -> error::Result<Self::TreeBuilder, Self::InnerError> {
        let objects = self.objects.write().expect("Could not write object");
        Ok(TestTreeBuilder {
            objects,
            oid: tree.oid,
        })
    }
}

impl Base for TestOdb {
    type Oid = TestOid;
    type InnerError = TestError;
}

pub struct TestTreeBuilder<'r> {
    objects: sync::RwLockWriteGuard<'r, HashSet<TestObject>>,
    oid: TestOid,
}

impl tree::Builder for TestTreeBuilder<'_> {
    type Oid = TestOid;
    type Error = TestError;

    fn write(mut self) -> Result<Self::Oid, Self::Error> {
        self.objects.insert(TestObject::Tree(TestTree {
            oid: self.oid.clone(),
        }));
        Ok(self.oid)
    }
}

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
