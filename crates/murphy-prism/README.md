# murphy-prism

`murphy-prism` is a small Murphy-maintained fork of the upstream
`ruby-prism` Rust crate.

The initial fork is based on `ruby-prism` 1.9.0 and keeps the public surface
close to upstream, with one Murphy-specific addition for linter and formatter
infrastructure: parsing while collecting Prism's source token stream.

The upstream project is available at <https://github.com/ruby/prism>.
