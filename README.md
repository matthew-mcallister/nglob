# nglob

nglob is a rust library for file globbing. It supports parallel async search,
file type filtering, and both Windows and Unix-like filesystems. It uses a
search algorithm based on tries and finite automata. It handles complex
patterns while having less surprising behavior than standard shell glob. The
API is intentionally simple to use.

## Limitations

- UTF-8 only currently
- glob patterns only, no regex
- Pattern base directory must be a literal, e.g. "C:\" and not "{C,D}:\"
