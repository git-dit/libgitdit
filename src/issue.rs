// git-dit - the distributed issue tracker for git
// Copyright (C) 2016, 2017 Matthias Beyer <mail@beyermatthias.de>
// Copyright (C) 2016, 2017 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//

//! Issues
//!
//! This module provides the `Issue` type and related functionality.
//!

use std::fmt::{self, Write};
use std::hash;

use crate::base::Base;
use crate::error::{self, ResultExt};
use crate::object::{commit, Database};
use crate::reference::{self, HEAD_COMPONENT};
use crate::remote;
use crate::traversal::{TraversalBuilder, Traversible};

/// Issue handle
///
/// Instances of this type represent single issues. Issues reside in
/// repositories and are uniquely identified by an id.
pub struct Issue<'r, R: Base> {
    repo: &'r R,
    id: R::Oid,
}

impl<'r, R: Base> Issue<'r, R> {
    /// Create a new handle for an issue with a given id
    ///
    /// This fn creates a new issue handle, without checking whether the issue
    /// itself exists.
    pub(crate) fn new_unchecked(repo: &'r R, id: R::Oid) -> Self {
        Self { repo, id }
    }

    /// Get the issue's id
    pub fn id(&self) -> &R::Oid {
        &self.id
    }

    /// Get the repository the issue lifes in
    pub(crate) fn repo(&self) -> &'r R {
        self.repo
    }
}

impl<'r, R: reference::Store<'r>> Issue<'r, R> {
    /// Get the local issue head for the issue
    ///
    /// Returns the head reference of the issue from the local repository, if
    /// present.
    pub fn local_head(&self) -> error::Result<Option<R::Reference>, R::InnerError> {
        let path = format!("refs/{DIT_REF_PART}/{}/{HEAD_COMPONENT}", self.id());
        self.repo().get_reference(path.as_ref())
    }

    /// Get local references for the issue
    ///
    /// Returns all references of a specific type associated with the issue from
    /// the local repository.
    pub fn local_refs(&self) -> error::Result<R::References, R::InnerError> {
        let path = format!("refs/{DIT_REF_PART}/{}", self.id());
        self.repo().references(path.as_ref())
    }

    /// Get the issue head for this issue for a specific remote
    ///
    /// Returns the head reference of the issue for a specific remote
    /// repository.
    pub fn remote_head(
        &self,
        remote: &impl remote::Name,
    ) -> error::Result<Option<R::Reference>, R::InnerError> {
        let make_err = || error::Kind::CannotFindIssueHead(self.id().clone());
        let mut path = remote.ref_path().wrap_with(make_err)?;
        write!(path, "/{DIT_REF_PART}/{}/{HEAD_COMPONENT}", self.id()).wrap_with(make_err)?;
        self.repo().get_reference(path.as_ref())
    }

    /// Get referernces for this issue for a specific remote
    ///
    /// Return all references of a specific type associated with the issue from
    /// a specific remote repository.
    pub fn remote_refs(
        &self,
        remote: &impl remote::Name,
    ) -> error::Result<R::References, R::InnerError> {
        let mut path = remote
            .ref_path()
            .wrap_with_kind(error::Kind::CannotConstructRevwalk)?;
        write!(path, "/{DIT_REF_PART}/{}", self.id())
            .wrap_with_kind(error::Kind::CannotConstructRevwalk)?;
        self.repo().references(path.as_ref())
    }

    /// Get remote heads for the issue
    pub fn all_remote_heads(
        &self,
    ) -> error::Result<
        impl Iterator<Item = error::Result<R::Reference, R::InnerError>> + '_,
        R::InnerError,
    > {
        let refs = self
            .repo()
            .remote_ref_paths()?
            .into_iter()
            .map(move |mut p| {
                write!(p, "/{DIT_REF_PART}/{}/{HEAD_COMPONENT}", self.id())
                    .wrap_with(|| error::Kind::CannotFindIssueHead(self.id().clone()))?;
                self.repo().get_reference(p.as_ref())
            })
            .filter_map(Result::transpose);
        Ok(refs)
    }

    /// Get remote references for the issue
    ///
    /// Return all references of a specific type associated with the issue from
    /// all remote repositories.
    pub fn all_remote_refs(
        &self,
    ) -> error::Result<impl Iterator<Item = Result<R::Reference, R::InnerError>>, R::InnerError>
    {
        use remote::Names;

        let ref_bases: Vec<_> = self
            .repo()
            .remote_names()?
            .ref_paths()
            .map(|p| {
                let mut path = p.wrap_with_kind(error::Kind::CannotConstructRevwalk)?;
                write!(path, "/{DIT_REF_PART}/{}", self.id())
                    .wrap_with_kind(error::Kind::CannotConstructRevwalk)?;
                self.repo().references(path.as_ref())
            })
            .collect::<Result<_, _>>()?;
        Ok(ref_bases.into_iter().flatten())
    }

    /// Get possible heads of the issue
    ///
    /// Returns the head references from both the local repository and remotes
    /// for this issue.
    pub fn all_heads(
        &self,
    ) -> error::Result<
        impl Iterator<Item = error::Result<R::Reference, R::InnerError>> + '_,
        R::InnerError,
    > {
        let refs = self
            .local_head()
            .transpose()
            .into_iter()
            .chain(self.all_remote_heads()?);
        Ok(refs)
    }

    /// Get references for the issue
    ///
    /// Return all references of a specific type associated with the issue from
    /// both the local and remote repositories.
    pub fn all_refs(
        &self,
    ) -> error::Result<impl Iterator<Item = Result<R::Reference, R::InnerError>>, R::InnerError>
    {
        let refs = self
            .local_refs()?
            .into_iter()
            .chain(self.all_remote_refs()?);
        Ok(refs)
    }

    /// Update the local head reference of the issue
    ///
    /// Updates the local head reference of the issue to the provided message.
    pub fn update_head(
        &self,
        message: R::Oid,
        replace: bool,
    ) -> error::Result<R::Reference, R::InnerError> {
        let path = format!("refs/{DIT_REF_PART}/{}/{HEAD_COMPONENT}", self.id());
        let reflogmsg = format!("git-dit: set head reference of {self} to {message}");
        self.repo()
            .set_reference(path.as_ref(), message, replace, &reflogmsg)
    }

    /// Add a new leaf reference associated with the issue
    ///
    /// Creates a new leaf reference for the message provided in the issue.
    pub fn add_leaf(&self, message: R::Oid) -> error::Result<R::Reference, R::InnerError> {
        use reference::LEAF_COMPONENT;

        let path = format!(
            "refs/{DIT_REF_PART}/{}/{LEAF_COMPONENT}/{message}",
            self.id(),
        );
        let reflogmsg = format!("git-dit: new leaf for {self}: {message}");
        self.repo()
            .set_reference(path.as_ref(), message, false, &reflogmsg)
    }
}

impl<'r, R: Database<'r>> Issue<'r, R> {
    /// Get the issue's initial message
    pub fn initial_message(&self) -> error::Result<R::Commit, R::InnerError> {
        self.repo().find_commit(self.id().clone())
    }

    /// Create a [commit::Builder] for messages for this issue
    ///
    /// The builder will be configured to add a new leaf reference pointing to
    /// the new message.
    pub fn message_builder<'c>(
        &self,
    ) -> error::Result<
        commit::Builder<'r, 'c, R, impl commit::FollowUp<'r, R, Output = R::Oid> + '_>,
        R::InnerError,
    >
    where
        R: reference::Store<'r>,
        'r: 'c,
    {
        self.repo()
            .commit_builder(move |_, o: R::Oid| self.add_leaf(o.clone()).map(|_| o))
    }

    /// Add a new message to the issue
    ///
    /// Adds a new message to the issue. Also create a leaf reference for the
    /// new message. Returns the message.
    pub fn add_message<'c, I>(
        &self,
        author: &R::Signature<'c>,
        committer: &R::Signature<'c>,
        message: &str,
        tree: &R::Tree,
        parents: I,
    ) -> error::Result<R::Commit, R::InnerError>
    where
        I: IntoIterator<Item = &'c R::Commit>,
        R: reference::Store<'r>,
        R::Commit: 'c,
    {
        use self::commit::Commit;

        let parents: Vec<_> = std::iter::FromIterator::from_iter(parents);

        let id = self
            .repo()
            .commit(author, committer, message, tree, parents.as_ref())?;
        let res = self.repo().find_commit(id)?;
        self.add_leaf(res.id())?;
        Ok(res)
    }
}

impl<'r, R: Database<'r> + Traversible<'r>> Issue<'r, R> {
    /// Get all messages of the issue
    pub fn messages(
        &self,
    ) -> error::Result<<R::TraversalBuilder as TraversalBuilder>::Iter, R::InnerError>
    where
        R: reference::Store<'r>,
    {
        use reference::Reference;

        self.all_refs()?
            .map(|r| r.wrap_with_kind(error::Kind::CannotGetReference))
            .filter_map(|r| r.map(|r| r.target()).transpose())
            .try_fold(self.terminated_messages()?, |m, r| {
                m.with_head(r?)
                    .map_err(Into::into)
                    .wrap_with_kind(error::Kind::CannotConstructRevwalk)
            })?
            .build()
            .map_err(Into::into)
            .wrap_with_kind(error::Kind::CannotConstructRevwalk)
    }

    /// Get messages of the issue starting from a specific one
    ///
    /// The [Iterator] returned will return all first parents up to and
    /// including the initial message of the issue.
    pub fn messages_from(
        &self,
        message: R::Oid,
    ) -> error::Result<<R::TraversalBuilder as TraversalBuilder>::Iter, R::InnerError> {
        self.terminated_messages()?
            .with_head(message)
            .and_then(TraversalBuilder::build)
            .map_err(Into::into)
            .wrap_with_kind(error::Kind::CannotConstructRevwalk)
    }

    /// Prepare a messages iterator which will terminate at the initial message
    pub fn terminated_messages(&self) -> error::Result<R::TraversalBuilder, R::InnerError> {
        use object::commit::Commit;

        self.repo()
            .traversal_builder()?
            .with_ends(self.initial_message()?.parent_ids())
            .map_err(Into::into)
            .wrap_with_kind(error::Kind::CannotConstructRevwalk)
    }
}

impl<R: Base> fmt::Display for Issue<'_, R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}", self.id())
    }
}

impl<R: Base> PartialEq for Issue<'_, R> {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl<R: Base> Eq for Issue<'_, R> {}

impl<R: Base> hash::Hash for Issue<'_, R> {
    fn hash<H>(&self, state: &mut H)
    where
        H: hash::Hasher,
    {
        self.id().hash(state);
    }
}

/// Reference part for the dit namespace
pub(crate) const DIT_REF_PART: &str = "dit";

#[cfg(test)]
mod tests {
    use super::*;

    use object::commit::Commit;
    use object::tests::TestOdb;
    use reference::tests::TestStore;
    use reference::Reference;

    type TestRepo = (TestStore, TestOdb);

    #[test]
    fn issue_leaves() {
        use reference::References;

        let repo = TestRepo::default();

        {
            // messages we're not supposed to see
            let initial_message = repo
                .commit_builder(TestRepo::find_commit)
                .expect("Cannot create commit builder")
                .build("Test message 1")
                .expect("Cannot create commit");
            let issue = Issue::new_unchecked(&repo, initial_message.id());
            issue
                .message_builder()
                .expect("Could not create builder")
                .with_parent(initial_message.clone())
                .build("Test message 2")
                .expect("Could not add message");
        }

        let initial_message = repo
            .commit_builder(TestRepo::find_commit)
            .expect("Cannot create commit builder")
            .build("Test message 1")
            .expect("Cannot create commit");
        let issue = Issue::new_unchecked(&repo, initial_message.id());
        let message = issue
            .message_builder()
            .expect("Could not create builder")
            .with_parent(initial_message.clone())
            .build("Test message 4")
            .expect("Could not add message");

        let mut leaves = issue
            .local_refs()
            .expect("Could not retrieve issue leaves")
            .leaves();
        let leaf = leaves
            .next()
            .expect("Could not find leaf reference")
            .expect("Could not retrieve leaf reference")
            .target()
            .expect("Could not determine the target of the leaf reference");
        assert_eq!(leaf, message);
        assert!(leaves.next().is_none());
    }

    #[test]
    fn local_refs() {
        let repo = TestRepo::default();

        {
            // messages we're not supposed to see
            let initial_message = repo
                .commit_builder(TestRepo::find_commit)
                .expect("Cannot create commit builder")
                .build("Test message 1")
                .expect("Cannot create commit");
            let issue = Issue::new_unchecked(&repo, initial_message.id());
            issue
                .update_head(initial_message.id(), true)
                .expect("Could not update head");
            issue
                .message_builder()
                .expect("Could not create builder")
                .with_parent(initial_message.clone())
                .build("Test message 2")
                .expect("Could not add message");
        }

        let initial_message = repo
            .commit_builder(TestRepo::find_commit)
            .expect("Cannot create commit builder")
            .build("Test message 2")
            .expect("Cannot create commit");
        let issue = Issue::new_unchecked(&repo, initial_message.id());
        issue
            .update_head(initial_message.id(), true)
            .expect("Could not update head");
        let message = issue
            .message_builder()
            .expect("Could not create builder")
            .with_parent(initial_message.clone())
            .build("Test message 3")
            .expect("Could not add message");

        let mut ids = vec![issue.id().clone(), message];
        ids.sort();
        let mut ref_ids: Vec<_> = issue
            .local_refs()
            .expect("Could not retrieve local refs")
            .into_iter()
            .map(|reference| reference.unwrap().target().unwrap())
            .collect();
        ref_ids.sort();
        assert_eq!(ref_ids, ids);
    }

    #[test]
    fn message_revwalk() {
        let repo = TestRepo::default();

        let initial_message1 = repo
            .commit_builder(TestRepo::find_commit)
            .expect("Cannot create commit builder")
            .build("Test message 1")
            .expect("Cannot create commit");
        let issue1 = Issue::new_unchecked(&repo, initial_message1.id());
        issue1
            .update_head(initial_message1.id(), true)
            .expect("Could not update head");

        let initial_message2 = repo
            .commit_builder(TestRepo::find_commit)
            .expect("Cannot create commit builder")
            .with_parent(initial_message1)
            .build("Test message 1")
            .expect("Cannot create commit");
        let issue2 = Issue::new_unchecked(&repo, initial_message2.id());
        issue2
            .update_head(initial_message2.id(), true)
            .expect("Could not update head");
        let message = issue2
            .message_builder()
            .expect("Could not create builder")
            .with_parent(initial_message2.clone())
            .build("Test message 3")
            .expect("Could not add message");

        let mut iter1 = issue1
            .messages()
            .expect("Could not create message revwalk iterator");
        let mut current_id = iter1
            .next()
            .expect("No more messages")
            .expect("Could not retrieve message");
        assert_eq!(current_id, issue1.id().clone());
        assert!(iter1.next().is_none());

        let mut iter2 = issue2
            .messages()
            .expect("Could not create message revwalk iterator");
        current_id = iter2
            .next()
            .expect("No more messages")
            .expect("Could not retrieve message");
        assert_eq!(current_id, message);

        current_id = iter2
            .next()
            .expect("No more messages")
            .expect("Could not retrieve message");
        assert_eq!(&current_id, issue2.id());

        assert_eq!(iter2.next(), None);
    }

    #[test]
    fn update_head() {
        let repo = TestRepo::default();

        let initial_message = repo
            .commit_builder(TestRepo::find_commit)
            .expect("Cannot create commit builder")
            .build("Test message 2")
            .expect("Cannot create commit");
        let issue = Issue::new_unchecked(&repo, initial_message.id());
        issue
            .update_head(initial_message.id(), true)
            .expect("Could not update head");

        let message = issue
            .message_builder()
            .expect("Could not create builder")
            .with_parent(initial_message.clone())
            .build("Test message 3")
            .expect("Could not add message");

        let mut local_head = issue
            .local_head()
            .expect("Could not retrieve local head")
            .expect("No local head found")
            .target()
            .expect("Could not get target of local head");
        assert_eq!(&local_head, issue.id());

        issue
            .update_head(message, true)
            .expect("Could not update head reference");
        local_head = issue
            .local_head()
            .expect("Could not retrieve local head")
            .expect("No local head found")
            .target()
            .expect("Could not get target of local head");
        assert_eq!(local_head, message);
    }
}

