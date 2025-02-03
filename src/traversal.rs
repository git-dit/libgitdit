// git-dit - the distributed issue tracker for git
// Copyright (C) 2024 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//! Commit/Message traversal

use crate::base::Base;
use crate::error::{self, ResultExt};

/// Entity containing commit graph information
///
/// A [Traversible] contains commit graph information and allows constructing an
/// [Iterator] for traversing this graph via a [TraversalBuilder].
pub trait Traversible<'t>: Base {
    /// [TraversalBuilder] type for this repository
    type TraversalBuilder: TraversalBuilder<
        Oid = Self::Oid,
        Error: Into<Self::InnerError>,
        BuildError: Into<Self::InnerError>,
    >;

    /// Get an [Iterator] yielding commits, following the chain of first parents
    ///
    /// This is a convenience function. It returns an [Iterator] over commits in
    /// reverse order, only following first parent commits.
    fn first_parent_messages(
        &'t self,
        id: Self::Oid,
    ) -> error::Result<<Self::TraversalBuilder as TraversalBuilder>::Iter, Self::InnerError> {
        self.traversal_builder()?
            .with_head(id)
            .and_then(TraversalBuilder::build)
            .map_err(Into::into)
            .wrap_with_kind(error::Kind::CannotConstructRevwalk)
    }

    /// Create a [TraversalBuilder]
    fn traversal_builder(&'t self) -> error::Result<Self::TraversalBuilder, Self::InnerError>;
}

impl<'t> Traversible<'t> for git2::Repository {
    type TraversalBuilder = git2::Revwalk<'t>;

    fn traversal_builder(&'t self) -> error::Result<Self::TraversalBuilder, Self::InnerError> {
        self.revwalk()
            .wrap_with_kind(error::Kind::CannotConstructRevwalk)
    }
}

/// Builder for a commit/message traversing [Iterator]
pub trait TraversalBuilder: Sized {
    /// Object id type associated with this traversal builder
    ///
    /// The type of object id is used for specifying tips and ends when building
    /// [Self::Iter]. It is also yielded by that [Iterator].
    type Oid;

    /// Error type used for the [Result]s yielded by [Self::Iter]
    type Error: std::error::Error;

    /// The [Iterator] type built by this builder
    type Iter: Iterator<Item = Result<Self::Oid, Self::Error>>;

    /// Error type used in builder functions' [Result]s
    ///
    /// Functions of this trait may return this error type
    type BuildError: std::error::Error;

    /// Add one head to the resulting [Self::Iter]
    ///
    /// The [Iterator] returned by [Self::build] will yield this [Self::Oid] and
    /// all their ancestors until an `end`.
    fn with_head(self, head: impl Into<Self::Oid>) -> Result<Self, Self::BuildError> {
        self.with_heads(std::iter::once(head))
    }

    /// Add heads to the resulting [Self::Iter]
    ///
    /// The [Iterator] returned by [Self::build] will yield these [Self::Oid]s
    /// and all their ancestors until an `end`.
    fn with_heads(
        self,
        heads: impl IntoIterator<Item = impl Into<Self::Oid>>,
    ) -> Result<Self, Self::BuildError>;

    /// Add one end to the resulting [Self::Iter]
    ///
    /// The [Iterator] returned by [Self::build] will never this [Self::Oid] and
    /// will not enqueue their parents.
    fn with_end(self, end: impl Into<Self::Oid>) -> Result<Self, Self::BuildError> {
        self.with_ends(std::iter::once(end))
    }

    /// Add ends to the resulting [Self::Iter]
    ///
    /// The [Iterator] returned by [Self::build] will never these [Self::Oid]s
    /// and will not enqueue their parents.
    fn with_ends(
        self,
        ends: impl IntoIterator<Item = impl Into<Self::Oid>>,
    ) -> Result<Self, Self::BuildError>;

    /// Build the [Iterator]
    fn build(self) -> Result<Self::Iter, Self::BuildError>;
}

impl TraversalBuilder for git2::Revwalk<'_> {
    type Oid = git2::Oid;

    type Iter = Self;

    type Error = git2::Error;

    type BuildError = git2::Error;

    fn with_heads(
        mut self,
        heads: impl IntoIterator<Item = impl Into<Self::Oid>>,
    ) -> Result<Self, Self::BuildError> {
        heads
            .into_iter()
            .try_for_each(|oid| self.push(oid.into()))?;
        Ok(self)
    }

    fn with_ends(
        mut self,
        ends: impl IntoIterator<Item = impl Into<Self::Oid>>,
    ) -> Result<Self, Self::BuildError> {
        ends.into_iter().try_for_each(|oid| self.hide(oid.into()))?;
        Ok(self)
    }

    fn build(mut self) -> Result<Self::Iter, Self::BuildError> {
        self.simplify_first_parent()?;
        self.set_sorting(git2::Sort::TOPOLOGICAL)?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::{self, HashSet};

    use crate::base::tests::TestOid;
    use crate::error::tests::TestError;
    use crate::object::tests::{TestObject, TestOdb};

    impl<'t, T> Traversible<'t> for (T, TestOdb)
    where
        T: Base<Oid = <TestOdb as Base>::Oid, InnerError = <TestOdb as Base>::InnerError>,
    {
        type TraversalBuilder = <TestOdb as Traversible<'t>>::TraversalBuilder;

        fn traversal_builder(&'t self) -> error::Result<Self::TraversalBuilder, Self::InnerError> {
            self.1.traversal_builder()
        }
    }

    impl<'t> Traversible<'t> for TestOdb {
        type TraversalBuilder = TestTraversal<'t>;

        fn traversal_builder(&'t self) -> error::Result<Self::TraversalBuilder, Self::InnerError> {
            Ok(TestTraversal {
                db: self.ro_objects(),
                heads: Default::default(),
                ends: Default::default(),
            })
        }
    }

    pub struct TestTraversal<'t> {
        db: std::sync::RwLockReadGuard<'t, HashSet<TestObject>>,
        heads: collections::BinaryHeap<TestOid>,
        ends: HashSet<TestOid>,
    }

    impl TraversalBuilder for TestTraversal<'_> {
        type Oid = TestOid;
        type Error = TestError;
        type Iter = Self;
        type BuildError = TestError;

        fn with_heads(
            mut self,
            heads: impl IntoIterator<Item = impl Into<Self::Oid>>,
        ) -> Result<Self, Self::BuildError> {
            self.heads.extend(heads.into_iter().map(Into::into));
            Ok(self)
        }

        fn with_ends(
            mut self,
            ends: impl IntoIterator<Item = impl Into<Self::Oid>>,
        ) -> Result<Self, Self::BuildError> {
            self.ends.extend(ends.into_iter().map(Into::into));
            Ok(self)
        }

        fn build(self) -> Result<Self::Iter, Self::BuildError> {
            Ok(self)
        }
    }

    impl Iterator for TestTraversal<'_> {
        type Item = Result<TestOid, TestError>;

        fn next(&mut self) -> Option<Self::Item> {
            use crate::object::commit::Commit;

            let id = self.heads.pop()?;

            let Some(object) = self.db.get(&id) else {
                return Some(Err(TestError));
            };

            let TestObject::Commit(commit) = object else {
                return Some(Err(TestError));
            };

            let ends = &self.ends;
            let parents = commit
                .parent_ids()
                .into_iter()
                .filter(|p| !ends.contains(&p));
            self.heads.extend(parents);

            // The same commit may be the parent of multiple commits we've
            // alreaty yielded. We don't check for duplicates when pushing
            // them.
            self.heads.retain(|c| c < &id);

            Some(Ok(id))
        }
    }
}
