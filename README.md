# Manalogue

Search engine for man pages because I wanted to "learn" rust.

To create indexing data, you need to [have](#sources) man pages and then put the path in executable in 'constants.rs'.

```sh
cargo run  --bin index
cargo run  --bin search -- make directory
```

## TODO

- [ ] Make web frontend.

## Sources

Man pages:     <https://git.kernel.org/pub/scm/docs/man-pages/man-pages.git>
GNU coreutils: <git://git.sv.gnu.org/coreutils>
