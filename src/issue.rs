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

use git2::{self, Commit, Oid, Reference, References};
use std::fmt;
use std::hash;
use std::result::Result as RResult;

use crate::base::Base;
use crate::error;
use crate::traversal::{TraversalBuilder, Traversible};
use error::*;
use error::Kind as EK;


#[derive(PartialEq)]
pub enum IssueRefType {
    Any,
    Head,
    Leaf,
}

impl IssueRefType {
    /// Get the part of a glob specific to the type
    ///
    pub fn glob_part(&self) -> &'static str {
        match *self {
            IssueRefType::Any   => "**",
            IssueRefType::Head  => "head",
            IssueRefType::Leaf  => "leaves/*",
        }
    }
}

impl fmt::Debug for IssueRefType {
    fn fmt(&self, f: &mut fmt::Formatter) -> RResult<(), fmt::Error> {
        f.write_str(match self {
            &IssueRefType::Any   => "Any ref",
            &IssueRefType::Head  => "Head ref",
            &IssueRefType::Leaf  => "Leaf ref",
        })
    }
}


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

impl<'r> Issue<'r, git2::Repository> {
    /// Get the issue's initial message
    ///
    pub fn initial_message(&self) -> Result<git2::Commit<'r>, git2::Error> {
        self.repo
            .find_commit(*self.id())
            .wrap_with(|| error::Kind::CannotGetCommitForRev(self.id().to_string()))
    }

    /// Get possible heads of the issue
    ///
    /// Returns the head references from both the local repository and remotes
    /// for this issue.
    ///
    pub fn heads(&self) -> Result<References<'r>, git2::Error> {
        let glob = format!("**/dit/{}/head", self.id());
        self.repo
            .references_glob(&glob)
            .wrap_with(|| EK::CannotFindIssueHead(*self.id()))
    }

    /// Get the local issue head for the issue
    ///
    /// Returns the head reference of the issue from the local repository, if
    /// present.
    ///
    pub fn local_head(&self) -> Result<Reference<'r>, git2::Error> {
        let refname = format!("refs/dit/{}/head", self.id());
        self.repo
            .find_reference(&refname)
            .wrap_with(|| EK::CannotFindIssueHead(*self.id()))
    }

    /// Get local references for the issue
    ///
    /// Return all references of a specific type associated with the issue from
    /// the local repository.
    ///
    pub fn local_refs(&self, ref_type: IssueRefType) -> Result<References<'r>, git2::Error> {
        let glob = format!("refs/dit/{}/{}", self.id(), ref_type.glob_part());
        self.repo
            .references_glob(&glob)
            .wrap_with_kind(EK::CannotGetReferences(glob))
    }

    /// Get remote references for the issue
    ///
    /// Return all references of a specific type associated with the issue from
    /// all remote repositories.
    ///
    pub fn remote_refs(&self, ref_type: IssueRefType) -> Result<References<'r>, git2::Error> {
        let glob = format!("refs/remotes/*/dit/{}/{}", self.id(), ref_type.glob_part());
        self.repo
            .references_glob(&glob)
            .wrap_with_kind(EK::CannotGetReferences(glob))
    }

    /// Get references for the issue
    ///
    /// Return all references of a specific type associated with the issue from
    /// both the local and remote repositories.
    ///
    pub fn all_refs(&self, ref_type: IssueRefType) -> Result<References<'r>, git2::Error> {
        let glob = format!("**/dit/{}/{}", self.id(), ref_type.glob_part());
        self.repo
            .references_glob(&glob)
            .wrap_with_kind(EK::CannotGetReferences(glob))
    }

    /// Get all messages of the issue
    pub fn messages(&self) -> Result<git2::Revwalk<'r>, git2::Error> {
        self.all_refs(IssueRefType::Any)?
            .map(|m| m?.peel(git2::ObjectType::Commit))
            .map(|m| m.wrap_with_kind(EK::CannotGetReference))
            .try_fold(self.terminated_messages()?, |b, m| {
                b.with_head(m?.id())
                    .wrap_with_kind(EK::CannotConstructRevwalk)
            })?
            .build()
            .wrap_with_kind(EK::CannotConstructRevwalk)
    }

    /// Get messages of the issue starting from a specific one
    ///
    /// The [Iterator] returned will return all first parents up to and
    /// including the initial message of the issue.
    pub fn messages_from(&self, message: Oid) -> Result<git2::Revwalk<'r>, git2::Error> {
        self.terminated_messages()?
            .with_head(message)
            .and_then(TraversalBuilder::build)
            .wrap_with_kind(EK::CannotConstructRevwalk)
    }

    /// Prepare a messages iterator which will terminate at the initial message
    pub fn terminated_messages(&self) -> Result<git2::Revwalk<'r>, git2::Error> {
        self.repo
            .traversal_builder()?
            .with_ends(self.initial_message()?.parent_ids())
            .wrap_with_kind(EK::CannotConstructRevwalk)
    }

    /// Add a new message to the issue
    ///
    /// Adds a new message to the issue. Also create a leaf reference for the
    /// new message. Returns the message.
    ///
    pub fn add_message<'a, A, I, J>(&self,
                                    author: &git2::Signature,
                                    committer: &git2::Signature,
                                    message: A,
                                    tree: &git2::Tree,
                                    parents: I
    ) -> Result<Commit<'r>, git2::Error>
        where A: AsRef<str>,
              I: IntoIterator<Item = &'a Commit<'a>, IntoIter = J>,
              J: Iterator<Item = &'a Commit<'a>>
    {
        let parent_vec : Vec<&Commit> = parents.into_iter().collect();

        self.repo
            .commit(None, author, committer, message.as_ref(), tree, &parent_vec)
            .and_then(|id| self.repo.find_commit(id))
            .wrap_with_kind(EK::CannotCreateMessage)
            .and_then(|message| self.add_leaf(message.id()).map(|_| message))
    }

    /// Update the local head reference of the issue
    ///
    /// Updates the local head reference of the issue to the provided message.
    ///
    /// # Warnings
    ///
    /// The function will update the reference even if it would not be an
    /// fast-forward update.
    ///
    pub fn update_head(&self, message: Oid, replace: bool) -> Result<Reference<'r>, git2::Error> {
        let refname = format!("refs/dit/{}/head", self.id());
        let reflogmsg = format!("git-dit: set head reference of {} to {}", self, message);
        self.repo
            .reference(&refname, message, replace, &reflogmsg)
            .wrap_with_kind(EK::CannotSetReference(refname))
    }

    /// Add a new leaf reference associated with the issue
    ///
    /// Creates a new leaf reference for the message provided in the issue.
    ///
    pub fn add_leaf(&self, message: Oid) -> Result<Reference<'r>, git2::Error> {
        let refname = format!("refs/dit/{}/leaves/{}", self.id(), message);
        let reflogmsg = format!("git-dit: new leaf for {}: {}", self, message);
        self.repo
            .reference(&refname, message, false, &reflogmsg)
            .wrap_with_kind(EK::CannotSetReference(refname))
    }
}

impl<R: Base> fmt::Display for Issue<'_, R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> RResult<(), fmt::Error> {
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
        where H: hash::Hasher
    {
        self.id().hash(state);
    }
}

/// Reference part for the dit namespace
pub(crate) const DIT_REF_PART: &str = "dit";

#[cfg(test)]
mod tests {
    use super::*;
    use test_utils::{TestingRepo, empty_tree};

    use repository::RepositoryExt;

    #[test]
    fn issue_leaves() {
        let mut testing_repo = TestingRepo::new("issue_leaves");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let empty_tree = empty_tree(repo);

        {
            // messages we're not supposed to see
            let issue = repo
                .create_issue(&sig, &sig, "Test message 1", &empty_tree, vec![])
                .expect("Could not create issue");
            let initial_message = issue
                .initial_message()
                .expect("Could not retrieve initial message");
            issue.add_message(&sig, &sig, "Test message 2", &empty_tree, vec![&initial_message])
                .expect("Could not add message");
        }

        let issue = repo
            .create_issue(&sig, &sig, "Test message 3", &empty_tree, vec![])
            .expect("Could not create issue");
        let initial_message = issue
            .initial_message()
            .expect("Could not retrieve initial message");
        let message = issue
            .add_message(&sig, &sig, "Test message 4", &empty_tree, vec![&initial_message])
            .expect("Could not add message");

        let mut leaves = issue
            .local_refs(IssueRefType::Leaf)
            .expect("Could not retrieve issue leaves");
        let leaf = leaves
            .next()
            .expect("Could not find leaf reference")
            .expect("Could not retrieve leaf reference")
            .target()
            .expect("Could not determine the target of the leaf reference");
        assert_eq!(leaf, message.id());
        assert!(leaves.next().is_none());
    }

    #[test]
    fn local_refs() {
        let mut testing_repo = TestingRepo::new("local_refs");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let empty_tree = empty_tree(repo);

        {
            // messages we're not supposed to see
            let issue = repo
                .create_issue(&sig, &sig, "Test message 1", &empty_tree, vec![])
                .expect("Could not create issue");
            let initial_message = issue
                .initial_message()
                .expect("Could not retrieve initial message");
            issue.add_message(&sig, &sig, "Test message 3", &empty_tree, vec![&initial_message])
                .expect("Could not add message");
        }

        let issue = repo
            .create_issue(&sig, &sig, "Test message 2", &empty_tree, vec![])
            .expect("Could not create issue");
        let initial_message = issue
            .initial_message()
            .expect("Could not retrieve initial message");
        let message = issue
            .add_message(&sig, &sig, "Test message 3", &empty_tree, vec![&initial_message])
            .expect("Could not add message");

        let mut ids = vec![issue.id().clone(), message.id()];
        ids.sort();
        let mut ref_ids: Vec<Oid> = issue
            .local_refs(IssueRefType::Any)
            .expect("Could not retrieve local refs")
            .map(|reference| reference.unwrap().target().unwrap())
            .collect();
        ref_ids.sort();
        assert_eq!(ref_ids, ids);
    }

    #[test]
    fn message_revwalk() {
        let mut testing_repo = TestingRepo::new("message_revwalk");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let empty_tree = empty_tree(repo);

        let issue1 = repo
            .create_issue(&sig, &sig, "Test message 1", &empty_tree, vec![])
            .expect("Could not create issue");
        let initial_message1 = issue1
            .initial_message()
            .expect("Could not retrieve initial message");

        let issue2 = repo
            .create_issue(&sig, &sig, "Test message 2", &empty_tree, vec![&initial_message1])
            .expect("Could not create issue");
        let initial_message2 = issue2
            .initial_message()
            .expect("Could not retrieve initial message");
        let message = issue2
            .add_message(&sig, &sig, "Test message 3", &empty_tree, vec![&initial_message2])
            .expect("Could not add message");
        let message_id = message.id();

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
        assert_eq!(current_id, message_id);

        current_id = iter2
            .next()
            .expect("No more messages")
            .expect("Could not retrieve message");
        assert_eq!(&current_id, issue2.id());

        assert_eq!(iter2.next(), None);
    }

    #[test]
    fn update_head() {
        let mut testing_repo = TestingRepo::new("update_head");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let empty_tree = empty_tree(repo);

        let issue = repo
            .create_issue(&sig, &sig, "Test message 2", &empty_tree, vec![])
            .expect("Could not create issue");
        let initial_message = issue
            .initial_message()
            .expect("Could not retrieve initial message");
        let message = issue
            .add_message(&sig, &sig, "Test message 3", &empty_tree, vec![&initial_message])
            .expect("Could not add message");

        let mut local_head = issue
            .local_head()
            .expect("Could not retrieve local head")
            .target()
            .expect("Could not get target of local head");
        assert_eq!(&local_head, issue.id());

        issue
            .update_head(message.id(), true)
            .expect("Could not update head reference");
        local_head = issue
            .local_head()
            .expect("Could not retrieve local head")
            .target()
            .expect("Could not get target of local head");
        assert_eq!(local_head, message.id());
    }
}

