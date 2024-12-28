// git-dit - the distributed issue tracker for git
// Copyright (C) 2016, 2017 Matthias Beyer <mail@beyermatthias.de>
// Copyright (C) 2016, 2017 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//

//! Repository related utilities
//!
//! This module provides the `RepositoryExt` extension trait which provides
//! issue handling utilities for repositories.
//!

use std::collections::HashSet;
use std::fmt;

use git2::{self, Commit, Oid, Tree};

use gc;
use issue::Issue;
use iter;
use traversal::TraversalBuilder;
use utils::ResultIterExt;

use error::*;
use error::{self, Kind as EK};


/// Set of unique issues
///
pub type UniqueIssues<'a> = HashSet<Issue<'a>>;


/// Extension trait for Repositories
///
/// This trait is intended as an extension for repositories. It introduces
/// utility functions for dealing with issues, e.g. for retrieving references
/// for issues, creating messages and finding the initial message of an issue.
pub trait RepositoryExt<'r> {
    /// Type used for representing Object IDs
    type Oid: Clone + fmt::Debug + fmt::Display;

    /// Type used for representing references
    type Reference<'a>;

    /// (Inner) error type associated with this repository
    type InnerError: for<'a> error::InnerError<Oid = Self::Oid, Reference<'a> = Self::Reference<'a>>;

    /// [TraversalBuilder] type for this repository
    type TraversalBuilder: TraversalBuilder<
        Oid = Self::Oid,
        Error: Into<Self::InnerError>,
        BuildError: Into<Self::InnerError>,
    >;

    /// Retrieve an issue
    ///
    /// Returns the issue with a given id.
    fn find_issue(&'r self, id: Self::Oid) -> Result<Issue<'r>, Self::InnerError>;

    /// Retrieve an issue by its head ref
    ///
    /// Returns the issue associated with a head reference.
    fn issue_by_head_ref(
        &'r self,
        head_ref: &Self::Reference<'_>,
    ) -> Result<Issue<'r>, Self::InnerError>;

    /// Find the issue with a given message in it
    ///
    /// Returns the issue containing the message provided
    fn issue_with_message(&'r self, message: &Commit) -> Result<Issue<'r>, Self::InnerError>;

    /// Get issue hashes for a prefix
    ///
    /// This function returns all known issues known to the DIT repo under the
    /// prefix provided (e.g. all issues for which refs exist under
    /// `<prefix>/dit/`). Provide "refs" as the prefix to get only local issues.
    fn issues_with_prefix(&'r self, prefix: &str) -> Result<UniqueIssues<'r>, Self::InnerError>;

    /// Get all issue hashes
    ///
    /// This function returns all known issues known to the DIT repo.
    fn issues(&'r self) -> Result<UniqueIssues<'r>, Self::InnerError>;

    /// Create a new issue with an initial message
    fn create_issue<'a, A, I, J>(
        &'r self,
        author: &git2::Signature,
        committer: &git2::Signature,
        message: A,
        tree: &Tree,
        parents: I,
    ) -> Result<Issue<'r>, Self::InnerError>
    where
        A: AsRef<str>,
        I: IntoIterator<Item = &'a Commit<'a>, IntoIter = J>,
        J: Iterator<Item = &'a Commit<'a>>;

    /// Get an revwalk configured as a first parent iterator
    ///
    /// This is a convenience function. It returns an iterator over messages in
    /// reverse order, only following first parents.
    fn first_parent_messages(
        &'r self,
        id: Self::Oid,
    ) -> Result<iter::Messages<'r>, Self::InnerError>;

    /// Produce a CollectableRefs
    fn collectable_refs(&'r self) -> gc::CollectableRefs<'r>;

    /// Create a [TraversalBuilder]
    fn traversal_builder(&'r self) -> Result<Self::TraversalBuilder, Self::InnerError>;
}

impl<'r> RepositoryExt<'r> for git2::Repository {
    type Oid = git2::Oid;
    type Reference<'a> = git2::Reference<'a>;
    type InnerError = git2::Error;
    type TraversalBuilder = git2::Revwalk<'r>;

    fn find_issue(&'r self, id: Self::Oid) -> Result<Issue<'r>, Self::InnerError> {
        let retval = Issue::new(self, id)?;

        // make sure the id refers to an issue by checking whether an associated
        // head reference exists
        if retval.heads()?.next().is_some() {
            Ok(retval)
        } else {
            Err(EK::CannotFindIssueHead(id).into())
        }
    }

    fn issue_by_head_ref(
        &'r self,
        head_ref: &Self::Reference<'_>,
    ) -> Result<Issue<'r>, Self::InnerError> {
        let name = head_ref.name();
        name.and_then(|name| if name.ends_with("/head") {
                Some(name)
            } else {
                None
            })
            .and_then(|name| name.rsplitn(3, "/").nth(1))
            .ok_or_else(|| {
                let n = name.unwrap_or_default().to_owned();
                EK::MalFormedHeadReference(n).into()
            })
            .and_then(|hash| {
               Oid::from_str(hash)
                   .wrap_with(|| EK::OidFormatError(hash.to_string()))
            })
            .and_then(|id| Issue::new(self, id))
    }

    fn issue_with_message(&'r self, message: &Commit) -> Result<Issue<'r>, Self::InnerError> {
        // follow the chain of first parents towards an initial message for
        // which a head exists
        for id in self.first_parent_messages(message.id())?.revwalk {
            let issue = self.find_issue(id?);
            if issue.is_ok() {
                return issue
            }
        }

        Err(EK::NoTreeInitFound(message.id()).into())
    }

    fn issues_with_prefix(&'r self, prefix: &str) -> Result<UniqueIssues<'r>, Self::InnerError> {
        let glob = format!("{}/dit/**/head", prefix);
        self.references_glob(&glob)
            .wrap_with_kind(EK::CannotGetReferences(glob))
            .map(|refs| iter::HeadRefsToIssuesIter::new(self, refs))?
            .collect_result()
    }

    fn issues(&'r self) -> Result<UniqueIssues<'r>, git2::Error> {
        let glob = "**/dit/**/head";
        self.references_glob(glob)
            .wrap_with(|| EK::CannotGetReferences(glob.to_owned()))
            .map(|refs| iter::HeadRefsToIssuesIter::new(self, refs))?
            .collect_result()
    }

    fn create_issue<'a, A, I, J>(
        &'r self,
        author: &git2::Signature,
        committer: &git2::Signature,
        message: A,
        tree: &Tree,
        parents: I,
    ) -> Result<Issue<'r>, Self::InnerError>
    where
        A: AsRef<str>,
        I: IntoIterator<Item = &'a Commit<'a>, IntoIter = J>,
        J: Iterator<Item = &'a Commit<'a>>,
    {
        let parent_vec : Vec<&Commit> = parents.into_iter().collect();

        self.commit(None, author, committer, message.as_ref(), tree, &parent_vec)
            .wrap_with_kind(EK::CannotCreateMessage)
            .and_then(|id| Issue::new(self, id))
            .and_then(|issue| {
                issue.update_head(issue.id(), true)?;
                Ok(issue)
            })
    }

    fn first_parent_messages(
        &'r self,
        id: Self::Oid,
    ) -> Result<iter::Messages<'r>, Self::InnerError> {
        iter::Messages::empty(self)
            .and_then(|mut messages| {
                messages.revwalk.push(id)?;
                messages.revwalk.simplify_first_parent().wrap_with_kind(EK::CannotConstructRevwalk)?;
                messages
                    .revwalk
                    .set_sorting(git2::Sort::TOPOLOGICAL)
                    .wrap_with_kind(EK::CannotConstructRevwalk)?;
                Ok(messages)
            })
    }

    fn collectable_refs(&'r self) -> gc::CollectableRefs<'r> {
        gc::CollectableRefs::new(self)
    }

    fn traversal_builder(&'r self) -> Result<Self::TraversalBuilder, Self::InnerError> {
        self.revwalk().wrap_with_kind(EK::CannotConstructRevwalk)
    }
}




#[cfg(test)]
mod tests {
    use super::*;
    use test_utils::{TestingRepo, empty_tree};

    // RepositoryExt tests

    #[test]
    fn find_issue() {
        let mut testing_repo = TestingRepo::new("find_issue");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let issue = repo
            .create_issue(&sig, &sig, "Test message 1", &empty_tree(repo), vec![])
            .expect("Could not create issue");

        repo.find_issue(issue.id())
            .expect("Could not tretrieve issue by id");
    }

    #[test]
    fn issue_by_head_ref() {
        let mut testing_repo = TestingRepo::new("issue_by_head_ref");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let issue = repo
            .create_issue(&sig, &sig, "Test message 1", &empty_tree(repo), vec![])
            .expect("Could not create issue");

        let local_head = issue
            .local_head()
            .expect("Could not retrieve local head reference");
        let retrieved_issue = repo
            .issue_by_head_ref(&local_head)
            .expect("Could not retrieve issue");
        assert_eq!(issue.id(), retrieved_issue.id());
    }

    #[test]
    fn issue_with_message() {
        let mut testing_repo = TestingRepo::new("issue_with_message");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let empty_tree = empty_tree(repo);
        let issue = repo
            .create_issue(&sig, &sig, "Test message 1", &empty_tree, vec![])
            .expect("Could not create issue");
        let initial_message = issue
            .initial_message()
            .expect("Could not retrieve initial message");
        let message = issue
            .add_message(&sig, &sig, "Test message 2", &empty_tree, vec![&initial_message])
            .expect("Could not add message");

        let retrieved_issue = repo
            .issue_with_message(&message)
            .expect("Could not retrieve issue");
        assert_eq!(retrieved_issue.id(), issue.id());
    }

    #[test]
    fn issues() {
        let mut testing_repo = TestingRepo::new("issues");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let issue = repo
            .create_issue(&sig, &sig, "Test message 1", &empty_tree(repo), vec![])
            .expect("Could not create issue");

        let mut issues = repo
            .issues()
            .expect("Could not retrieve issues")
            .into_iter();
        let retrieved_issue = issues
            .next()
            .expect("Could not retrieve issue");
        assert_eq!(retrieved_issue.id(), issue.id());
        assert!(issues.next().is_none());
    }

    #[test]
    fn first_parent_messages() {
        let mut testing_repo = TestingRepo::new("first_parent_revwalk");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let empty_tree = empty_tree(repo);
        let issue = repo
            .create_issue(&sig, &sig, "Test message 1", &empty_tree, vec![])
            .expect("Could not create issue");
        let initial_message = issue
            .initial_message()
            .expect("Could not retrieve initial message");
        let message = issue
            .add_message(&sig, &sig, "Test message 2", &empty_tree, vec![&initial_message])
            .expect("Could not add message");

        let mut iter = repo
            .first_parent_messages(message.id())
            .expect("Could not create first parent iterator");
        assert_eq!(iter.next().unwrap().unwrap().id(), message.id());
        assert_eq!(iter.next().unwrap().unwrap().id(), issue.id());
        assert!(iter.next().is_none());
    }
}

