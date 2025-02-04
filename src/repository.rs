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

use crate::error::{self, ResultExt};
use crate::object::{self, commit};
use crate::reference;
use crate::traversal::Traversible;
use issue::Issue;

/// Set of unique issues
pub type UniqueIssues<'r, R> = HashSet<Issue<'r, R>>;


/// Extension trait for Repositories
///
/// This trait is intended as an extension for repositories. It introduces
/// utility functions for dealing with issues, e.g. for retrieving references
/// for issues, creating messages and finding the initial message of an issue.
pub trait RepositoryExt<'r>: reference::Store<'r> + Sized {
    /// Retrieve an issue
    ///
    /// Returns the issue with a given id.
    fn find_issue(&'r self, id: Self::Oid) -> error::Result<Issue<'r, Self>, Self::InnerError> {
        let retval = Issue::new_unchecked(self, id.clone());

        // We need to make sure the id refers to an issue by checking whether an
        // associated head reference exists. And we do not want to pessimise the
        // case where we have a local reference.
        if retval.local_head()?.is_none() {
            retval
                .all_remote_heads()?
                .next()
                .ok_or(error::Kind::CannotFindIssueHead(id))??;
        }

        Ok(retval)
    }

    /// Retrieve an issue by its head ref
    ///
    /// Returns the issue associated with a head reference.
    fn issue_by_head_ref(
        &'r self,
        head_ref: &Self::Reference,
    ) -> error::Result<Issue<'r, Self>, Self::InnerError> {
        use reference::Reference;

        head_ref
            .parts()
            .filter(|p| p.kind == reference::Kind::Head)
            .map(|p| Issue::new_unchecked(self, p.issue))
            .ok_or_else(|| match head_ref.name() {
                Ok(s) => error::Kind::MalFormedHeadReference(s.to_owned()).into(),
                Err(e) => error::Kind::CannotGetReference.wrap(e),
            })
    }

    /// Find the issue with a given message in it
    ///
    /// Returns the issue containing the message provided
    fn issue_with_message(
        &'r self,
        message: Self::Oid,
    ) -> error::Result<Issue<'r, Self>, Self::InnerError>
    where
        Self: Traversible<'r>,
    {
        for message in self.first_parent_messages(message.clone())? {
            let message = message
                .map_err(Into::into)
                .wrap_with_kind(error::Kind::CannotGetCommit)?;
            if let Ok(issue) = self.find_issue(message) {
                return Ok(issue);
            }
        }

        Err(error::Kind::NoTreeInitFound(message).into())
    }

    /// Get issue hashes for a prefix
    ///
    /// This function returns all known issues known to the DIT repo under the
    /// prefix provided (e.g. all issues for which refs exist under
    /// `<prefix>/dit/`). Provide "refs" as the prefix to get only local issues.
    fn issues_with_prefix(
        &'r self,
        prefix: &str,
    ) -> error::Result<
        impl IntoIterator<Item = error::Result<Issue<'r, Self>, Self::InnerError>>,
        Self::InnerError,
    > {
        use issue::DIT_REF_PART;
        use reference::Reference;

        let path = format!("{prefix}/{DIT_REF_PART}");
        let res = self
            .references(path.as_ref())?
            .into_iter()
            .map(move |r| {
                let issue = r
                    .wrap_with_kind(error::Kind::CannotGetReference)?
                    .parts()
                    .filter(|p| p.kind == reference::Kind::Head)
                    .map(|p| Issue::new_unchecked(self, p.issue));
                Ok(issue)
            })
            .flat_map(Result::transpose);
        Ok(res)
    }

    /// Get all issue hashes
    ///
    /// This function returns all known issues known to the DIT repo.
    fn issues(&'r self) -> error::Result<UniqueIssues<'r, Self>, Self::InnerError> {
        use std::iter::FromIterator;

        use remote::Names;

        let mut issues: UniqueIssues<_> = Result::from_iter(self.issues_with_prefix("refs")?)?;
        self.remote_names()?.ref_paths().try_for_each(|p| {
            let path = p.wrap_with_kind(error::Kind::CannotConstructRevwalk)?;
            for issue in self.issues_with_prefix(path.as_ref())? {
                issues.insert(issue?);
            }
            Ok::<_, error::Error<Self::InnerError>>(())
        })?;
        Ok(issues)
    }

    /// Create a builder for issues
    fn issue_builder<'c>(
        &'r self,
    ) -> error::Result<
        commit::Builder<'r, 'c, Self, impl commit::FollowUp<'r, Self, Output = Issue<'r, Self>>>,
        Self::InnerError,
    >
    where
        Self: object::Database<'r>,
        'r: 'c,
    {
        self.commit_builder(|r, o: Self::Oid| {
            let issue = Issue::new_unchecked(r, o.clone());
            issue.update_head(o, false)?;
            Ok(issue)
        })
    }
}

impl RepositoryExt<'_> for git2::Repository {}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::object::tests::TestOdb;
    use crate::reference::tests::TestStore;

    type TestRepo = (TestStore, TestOdb);

    impl RepositoryExt<'_> for TestRepo {}

    #[test]
    fn find_issue() {
        let repo = TestRepo::default();

        let issue = repo
            .issue_builder()
            .expect("Could not create issue builder")
            .build("Test message 1")
            .expect("Could not create issue");

        repo.find_issue(issue.id().clone())
            .expect("Could not tretrieve issue by id");
    }

    #[test]
    fn issue_by_head_ref() {
        let repo = TestRepo::default();

        let issue = repo
            .issue_builder()
            .expect("Could not create issue builder")
            .build("Test message 1")
            .expect("Could not create issue");

        let local_head = issue
            .local_head()
            .expect("No local head found")
            .expect("Could not retrieve local head reference");
        let retrieved_issue = repo
            .issue_by_head_ref(&local_head)
            .expect("Could not retrieve issue");
        assert_eq!(issue.id(), retrieved_issue.id());
    }

    #[test]
    fn issue_with_message() {
        let repo = TestRepo::default();

        let issue = repo
            .issue_builder()
            .expect("Could not create issue builder")
            .build("Test message 1")
            .expect("Could not create issue");
        let initial_message = issue
            .initial_message()
            .expect("Could not retrieve initial message");
        let message = issue
            .message_builder()
            .expect("Could not create builder")
            .with_parent(initial_message.clone())
            .build("Test message 2")
            .expect("Could not add message");

        let retrieved_issue = repo
            .issue_with_message(message)
            .expect("Could not retrieve issue");
        assert_eq!(retrieved_issue.id(), issue.id());
    }

    #[test]
    fn issues() {
        let repo = TestRepo::default();

        let issue = repo
            .issue_builder()
            .expect("Could not create issue builder")
            .build("Test message 1")
            .expect("Could not create issue");

        let mut issues = repo
            .issues()
            .expect("Could not retrieve issues")
            .into_iter();
        let retrieved_issue = issues.next().expect("Could not retrieve issue");
        assert_eq!(retrieved_issue.id(), issue.id());
        assert!(issues.next().is_none());
    }

    #[test]
    fn first_parent_messages() {
        let repo = TestRepo::default();

        let issue = repo
            .issue_builder()
            .expect("Could not create issue builder")
            .build("Test message 1")
            .expect("Could not create issue");
        let initial_message = issue
            .initial_message()
            .expect("Could not retrieve initial message");
        let message = issue
            .message_builder()
            .expect("Could not create builder")
            .with_parent(initial_message.clone())
            .build("Test message 2")
            .expect("Could not add message");

        let mut iter = repo
            .first_parent_messages(message)
            .expect("Could not create first parent iterator");
        let mut current_id = iter
            .next()
            .expect("No more messages")
            .expect("Could not retrieve message");
        assert_eq!(current_id, message);

        current_id = iter
            .next()
            .expect("No more messages")
            .expect("Could not retrieve message");
        assert_eq!(&current_id, issue.id());

        assert_eq!(iter.next(), None);
    }
}

