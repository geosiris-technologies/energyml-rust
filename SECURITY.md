# Security policy

## Reporting a vulnerability

Please report security problems **privately**. Do not open a public issue or pull request for them.

- Use GitHub's private vulnerability reporting: the **Security** tab of this repository, then **Report a vulnerability**.
- Or write to: TODO(verify): security contact

Please include the version, the platform, a description of the problem and, if you can, a minimal file or command that reproduces it. Packages that crash the reader (malformed zip, XML or HDF5) are in scope.

We aim to acknowledge a report within a few working days, and to tell you what we decide and when a fix will ship. TODO(verify): confirm the response times the maintainers can commit to.

## Supported versions

Until 1.0, only the latest released version receives security fixes.

## Scope

This policy covers the code in this repository. The generated crates under `crates/generated/` are in scope as built artifacts; the Energistics schemas they come from are not ours to change.
