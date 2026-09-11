# nglob

nglob is a rust library for file globbing. It supports multiple async runtimes,
multiple search options, and both Windows and Unix-like filesystems. It sports
a search algorithm based on finite automata which results in simple semantics
while handling complex patterns.

## Limitations

- nglob currently only works on UTF-8 filesystems.
- nglob does not do cycle detection, although it will stop searching past a
  maximum recursion depth.
