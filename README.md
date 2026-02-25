# Manalogue

Search engine for man pages because I wanted to "learn" rust.

To create indexing data, you need to [have](#sources) man pages and then put the path in executable in 'constants.rs'.

## Usage

```sh
# Start reading man pages to index them, this will override previous index, change file name in man.idx
cargo run  --bin index
# Search a query
cargo run  --bin search -- make directory
# Start sever, I read man pages from system using 'man command'
cargo run  --bin server
```

## TODO

- [X] Make web frontend.

## Sources

Man pages:     <https://git.kernel.org/pub/scm/docs/man-pages/man-pages.git>

GNU coreutils: <git://git.sv.gnu.org/coreutils>
