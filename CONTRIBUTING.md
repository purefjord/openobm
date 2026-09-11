# Contributing

Run the build and test commands in the README before submitting a change.
Describe the behavior you changed and the checks you actually ran. Report
development comparisons as evidence for the cases tested, and identify any
private inputs a result requires.

## Keep private material local

- Use your GitHub-provided noreply address for both commit authorship and
  committer metadata. Set it with `git config --local user.email` followed by
  the address from your GitHub email settings.
- Keep AI session links, conversation transcripts, personal paths, credentials,
  extracted game data, reference captures, and local saves out of commits.
- Review `git diff --cached` and `git diff --cached --name-status` before
  committing. Review images visually, including their metadata.
- Stage named files. Do not force-add ignored development material.

With Node.js 18 or newer installed, enable this repository's local hooks:

```sh
git config --local core.hooksPath .githooks
node tools/check-publication.cjs --history
```

The hooks check staged files, commit messages, identities, and reachable history
before a push. CI repeats the history check. These are targeted checks for common
publication mistakes, not a guarantee that all secrets or private content will
be detected. They do not validate credentials or contact their providers.

Hooks are configured separately for each clone. If you already use a custom hook
setup, integrate the checks there before changing `core.hooksPath`.

Keep AI assistance attribution brief and factual. Session URLs and private
transcripts are not needed for attribution.
