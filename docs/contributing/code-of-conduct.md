# Code of Conduct

## Our Pledge

We as members, contributors, and leaders pledge to make participation in the TurboMCP community a harassment-free experience for everyone, regardless of age, body size, visible or invisible disability, ethnicity, sex characteristics, gender identity and expression, level of experience, education, socio-economic status, nationality, personal appearance, race, religion, or sexual identity and orientation.

We pledge to act and interact in ways that contribute to an open, welcoming, diverse, inclusive, and healthy community.

## Our Standards

### Examples of Behavior That Contributes to a Positive Environment

- **Be Respectful** - Demonstrating empathy and kindness toward other people
- **Be Constructive** - Providing and gracefully accepting constructive feedback
- **Be Collaborative** - Focusing on what is best for the community
- **Be Professional** - Using welcoming and inclusive language
- **Show Gratitude** - Acknowledging the contributions of others
- **Be Patient** - Understanding that people have different levels of experience
- **Be Open-Minded** - Being receptive to different viewpoints and experiences

**Examples:**

- "Thanks for the PR! I have some suggestions on the error handling approach..."
- "I see what you're trying to do. Have you considered using the `Result` type here?"
- "Great question! Let me explain how the dependency injection system works..."
- "I appreciate you taking the time to review this. Your feedback helped improve the design."

### Examples of Unacceptable Behavior

- **Harassment** - Trolling, insulting or derogatory comments, and personal or political attacks
- **Discrimination** - Public or private harassment based on protected characteristics
- **Inappropriate Content** - Sexual language, imagery, or unwelcome sexual attention
- **Privacy Violations** - Publishing others' private information without explicit permission
- **Professional Misconduct** - Conduct that could reasonably be considered inappropriate in a professional setting
- **Bad Faith** - Deliberately derailing discussions or wasting others' time

**Examples:**

- Insulting a contributor's coding skills
- Making jokes about someone's English proficiency
- Dismissing bug reports without investigation
- Demanding immediate responses to questions
- Repeatedly arguing after a decision has been made

## Scope

This Code of Conduct applies within all community spaces, including:

- GitHub repositories (issues, pull requests, discussions)
- Discord server (if applicable)
- Email communications
- Social media when representing the project
- Community events (virtual or in-person)

This Code of Conduct also applies when an individual is officially representing the community in public spaces.

## Enforcement Responsibilities

Community leaders are responsible for clarifying and enforcing our standards of acceptable behavior and will take appropriate and fair corrective action in response to any behavior that they deem inappropriate, threatening, offensive, or harmful.

Community leaders have the right and responsibility to remove, edit, or reject comments, commits, code, wiki edits, issues, and other contributions that are not aligned with this Code of Conduct, and will communicate reasons for moderation decisions when appropriate.

## Reporting

### How to Report

If you experience or witness unacceptable behavior, please report it by:

1. **GitHub**: Open a confidential security advisory at https://github.com/turbomcp/turbomcp/security/advisories
2. **Email**: Send a detailed report to conduct@turbomcp.dev (if available)
3. **Direct Message**: Contact project maintainers directly

### What to Include in a Report

Please include:

- Your contact information
- Names (real, nicknames, or pseudonyms) of any individuals involved
- Description of the incident (be specific)
- Approximate time and date
- Screenshots or logs (if applicable)
- Any additional context

**All reports will be handled with discretion and confidentiality.**

### Response Timeline

- **Initial Response**: Within 48 hours
- **Investigation**: Within 7 days
- **Resolution**: Within 14 days (complex cases may take longer)

You will receive updates throughout the process.

## Enforcement Guidelines

Community leaders will follow these Community Impact Guidelines in determining the consequences for any action they deem in violation of this Code of Conduct:

### 1. Correction

**Community Impact**: Use of inappropriate language or other behavior deemed unprofessional or unwelcome in the community.

**Consequence**: A private, written warning from community leaders, providing clarity around the nature of the violation and an explanation of why the behavior was inappropriate. A public apology may be requested.

**Example**: Using insensitive language in a code review comment.

### 2. Warning

**Community Impact**: A violation through a single incident or series of actions.

**Consequence**: A warning with consequences for continued behavior. No interaction with the people involved, including unsolicited interaction with those enforcing the Code of Conduct, for a specified period of time. This includes avoiding interactions in community spaces as well as external channels like social media. Violating these terms may lead to a temporary or permanent ban.

**Example**: Repeatedly derailing technical discussions with off-topic arguments.

### 3. Temporary Ban

**Community Impact**: A serious violation of community standards, including sustained inappropriate behavior.

**Consequence**: A temporary ban from any sort of interaction or public communication with the community for a specified period of time. No public or private interaction with the people involved, including unsolicited interaction with those enforcing the Code of Conduct, is allowed during this period. Violating these terms may lead to a permanent ban.

**Example**: Harassing another contributor or making discriminatory remarks.

### 4. Permanent Ban

**Community Impact**: Demonstrating a pattern of violation of community standards, including sustained inappropriate behavior, harassment of an individual, or aggression toward or disparagement of classes of individuals.

**Consequence**: A permanent ban from any sort of public interaction within the community.

**Example**: Doxxing, sustained harassment, or threats of violence.

## Community Guidelines

### Technical Discussions

**Do:**
- Focus on technical merit
- Provide constructive feedback
- Ask clarifying questions
- Share knowledge generously
- Acknowledge good work

**Don't:**
- Make it personal
- Dismiss ideas without explanation
- Use absolutes ("This will never work")
- Gatekeep based on experience level
- Derail discussions

### Code Reviews

**Do:**
- Review the code, not the person
- Explain the "why" behind suggestions
- Offer alternatives
- Appreciate the effort
- Test changes when possible

**Don't:**
- Nitpick without explanation
- Block PRs for minor style issues
- Demand perfect code
- Make assumptions about knowledge
- Review without running the code

**Example - Good Review:**

```markdown
Thanks for this PR! The overall approach looks solid. A few suggestions:

1. Consider using `match` instead of multiple `if let` chains for better readability
2. The error handling in line 45 could use more context - maybe wrap with `.context()`?
3. Great test coverage! One edge case to add: what happens with empty input?

Let me know if you'd like help with any of these.
```

**Example - Bad Review:**

```markdown
This code is a mess. You clearly don't understand Rust.
- Line 10: Wrong
- Line 20: Bad
- Line 30: No

Please rewrite this completely.
```

### Issue Reporting

**Do:**
- Provide a clear description
- Include reproduction steps
- Share error messages
- Specify environment details
- Search for duplicates first

**Don't:**
- Demand immediate fixes
- Report multiple issues in one ticket
- Use ALL CAPS
- Include irrelevant information
- Blame maintainers

**Example - Good Issue:**

```markdown
## Bug: Server crashes on malformed JSON-RPC request

**Description**: Server panics when receiving a request with invalid `jsonrpc` field.

**Steps to Reproduce**:
1. Send request: `{"jsonrpc": "1.0", "method": "tools/list", "id": 1}`
2. Server panics with error: `thread 'main' panicked at 'invalid version'`

**Expected**: Should return JSON-RPC error with code -32600

**Environment**:
- TurboMCP version: 2.1.1
- Rust version: 1.89.0
- OS: Ubuntu 22.04

**Stack Trace**:
```
[paste stack trace]
```

### Pull Requests

**Do:**
- Write clear commit messages
- Add tests for new features
- Update documentation
- Keep changes focused
- Respond to feedback promptly

**Don't:**
- Submit massive PRs
- Mix unrelated changes
- Break existing tests
- Ignore CI failures
- Force-push after review

**Example - Good PR Description:**

```markdown
## Add SIMD JSON parsing support

This PR adds optional SIMD-accelerated JSON parsing using the `simd-json` crate.

**Changes**:
- Added `simd` feature flag
- Conditional compilation for SIMD vs standard parsing
- Benchmarks showing 2.7x performance improvement
- Documentation updates

**Testing**:
- All existing tests pass
- Added benchmark suite
- Tested with/without SIMD feature

**Breaking Changes**: None

Closes #123
```

## Mentorship & Learning

We actively encourage learning and growth:

**For Beginners:**
- Label beginner-friendly issues with `good-first-issue`
- Provide detailed reproduction steps
- Offer guidance in code reviews
- Be patient with questions

**For Mentors:**
- Share knowledge freely
- Explain concepts clearly
- Encourage experimentation
- Celebrate progress

**For Everyone:**
- Ask questions when unsure
- Admit mistakes openly
- Learn from feedback
- Help others learn

## Diversity & Inclusion

TurboMCP is committed to diversity and inclusion:

- **Language**: We communicate primarily in English, but we welcome contributors from all language backgrounds. Be patient with non-native speakers.
- **Experience**: We value contributions from developers at all levels, from beginners to experts.
- **Geography**: We have contributors worldwide - be mindful of time zones.
- **Accessibility**: We strive to make our documentation and tools accessible to everyone.

### Making the Community Inclusive

- Use gender-neutral language ("they/them" instead of "he/she")
- Avoid assumptions about knowledge or background
- Provide context for jargon and acronyms
- Welcome questions from newcomers
- Respect cultural differences

## Recognition

We believe in recognizing contributions:

### Types of Contributions We Value

- Code contributions (features, bug fixes)
- Documentation improvements
- Bug reports and feature requests
- Code reviews
- Answering questions
- Community support
- Design feedback
- Performance testing

### How We Recognize Contributors

- Contributors listed in CONTRIBUTORS.md
- Acknowledgment in release notes
- Shoutouts in community channels
- Invitation to contributor meetings

## Amendments

This Code of Conduct may be amended by the project maintainers. Material changes will be announced to the community.

**Version**: 1.0
**Last Updated**: 2025-12-10
**Effective Date**: 2025-12-10

## Attribution

This Code of Conduct is adapted from:
- [Contributor Covenant](https://www.contributor-covenant.org/), version 2.1
- [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct)
- [OpenSSF Best Practices](https://www.bestpractices.dev/)

## Questions?

If you have questions about this Code of Conduct, please:

1. Read the full document carefully
2. Check the [FAQ](../faq.md) for common questions
3. Open a GitHub Discussion
4. Contact maintainers directly

## Summary

**In short:**
- Be respectful and professional
- Provide constructive feedback
- Welcome newcomers
- Focus on technical merit
- Report violations confidentially
- Learn and help others learn

Thank you for helping make TurboMCP a welcoming, inclusive community!

## Contact

- **GitHub**: https://github.com/turbomcp/turbomcp
- **Issues**: https://github.com/turbomcp/turbomcp/issues
- **Discussions**: https://github.com/turbomcp/turbomcp/discussions
- **Security**: https://github.com/turbomcp/turbomcp/security/advisories

## License

This Code of Conduct is distributed under a [Creative Commons Attribution-ShareAlike 4.0 International License](https://creativecommons.org/licenses/by-sa/4.0/).
