# nglob

nglob is an implementation of glob file search as a Rust library. It supports
multiple async runtimes, a variety of search options, and both Windows and
Unix-like filesystems. It uses a search algorithm based on nondeterministic
finite automata, which simultaneously handles complex search patterns and
filesystem-specific quirks while minimizing the number of directories visited.
