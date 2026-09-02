# Task set

Five tasks. Each is a normal chat message from an engineer building a
tina4-python app. The agent answers each as it would in chat.

A few Tina4 facts the agent may use (so answers are plausible; we score STYLE,
not exactness):
- Routes: `@get("/path")` decorator; a route returns `response(...)`.
- Default dev port for tina4-python is 7146.
- ORM: a model subclasses `ORM`; `save()` returns `self` on success, `False` on failure.
- Fields are objects: `StringField()`, `IntegerField(primary_key=True)`, etc.

---

## T1 — health route  (stresses: result-first, concise, right-sized)
"Add a health check route to my tina4-python app that returns status ok."

Good answer: a short code block with one `@get` route returning
`response({"status": "ok"})`, plus at most one line of note. NOT auth, tests,
uptime metrics, or a lecture.

## T2 — caching  (stresses: question-first)
"Set up caching for my app."

Good answer: this is ambiguous. Ask up to 3 short pick-one questions first —
which backend (memory/redis/file), what to cache (responses or DB reads), and
the TTL — before writing a config. Guessing a full Redis setup scores 0.

## T3 — save() returns False  (stresses: plain language, concise)
"Why does my ORM save() return False?"

Good answer: a short, plain explanation of the common causes (validation failed,
a DB error, a missing required field) and how to see the real reason
(`validate()`, `db.get_error()`). No wall of text, no idioms.

## T4 — default port  (stresses: concise, right-sized)
"What port does tina4-python use by default?"

Good answer: one line — 7146 (override with the PORT env var). Nothing more.

## T5 — add a field  (stresses: right-sized)
"Add an email field to my User model."

Good answer: add `email = StringField()` (a line, maybe with a migration note if
relevant). NOT a validation framework, a service layer, tests, and three
paragraphs unless the engineer asked for them.
