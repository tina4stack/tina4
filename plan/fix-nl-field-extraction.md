# Thread 8 — NL → scaffold: extract fields + stop phantom resources

## Repro (live playground build, 2026-07-13)
"Build a products resource with name and price fields" → the agent scaffolded a
`Product` model + `product` migration, but the migration was a bare skeleton
(`id`, `created_at`) — **no `name`, no `price`**. It also scaffolded a phantom
`Automatically` resource from the planner's prose ("…handle the product resource
**automatically**").

## Root cause
- `scaffold_first` (agent.rs) calls `run_framework_generate(_, "model", m, &[])`
  — passes NO `--fields`. The framework generator DOES support
  `generate model Product --fields "name:string,price:float"` (populates model
  AND migration AND emits field tests), but the agent never extracts or passes
  the fields.
- `detect_resource_name` mines resource nouns out of arbitrary prose, so the
  planner's own wording ("automatically") became a second resource.

## Scope
- [ ] `detect_fields(ctx) -> Vec<(String,String)>` — parse field names from NL
      ("with name and price fields", "having a title and a body", explicit
      "name:string, price:decimal"). Type inference:
      price/cost/amount/total/rate → `float`; count/qty/quantity/age/number/stock
      → `int`; is_/has_/active/enabled/flag → `bool`; date/_at/_on/time → `datetime`;
      else → `string`. Emit generator-compatible type tokens.
- [ ] Thread fields into `scaffold_first` → `run_framework_generate("model", m,
      ["--fields", "<spec>"])` when fields were detected.
- [ ] Tighten `detect_resource_name`: drop adverbs (…ly) and planner-boilerplate
      stopwords; don't treat words from a step-list as resources.

## Tests (real — no mocks)
- [ ] Rust unit: `detect_fields("... with name and price fields")` →
      `[(name,string),(price,float)]`; type-inference cases; explicit `x:int`.
- [ ] Rust unit: `detect_resource_name("...handle the product resource
      automatically")` → `Some("Product")`, NOT "Automatically".
- [ ] Live: rebuild agent, re-run the playground build → `create_product.sql`
      has `name` + `price` columns; the `product` table schema has them; the
      model matches; no phantom resource scaffolded.

## Bugs
- [ ] Requested fields never reach the schema (skeleton migration).
- [ ] Model/migration inconsistent (model had stray `name`, table did not).
- [ ] Phantom resource from planner prose.

## Verification
- [x] 7 Rust unit tests (detect_fields name/price, type inference, string-forced-
      over-numeric, explicit types, no-clause; detect_resource ignores adverb,
      still finds plain noun). Full suite 110 pass, clippy clean.
- [x] Live playground: `POST /execute` a products plan → `create_product.sql`,
      `Product.py`, and the actual `product` table ALL have `name` (TEXT/StringField)
      + `price` (REAL/NumericField). 5 co-emitted tests passed; migrated+reloaded.
      **No phantom `Automatically` resource.** `GET /api/products` →
      `{"records":[{"id":1,"name":"Sprocket","price":9.99,...}]}` live.

## Thread 8b — the plan-driven flow (found by live end-to-end testing)
The first fix only worked when the ctx reaching `scaffold_first` carried the
fields. In the REAL Threads-pane flow the planner rewrites the request into
steps that drop them ("Use the generator to create a model named Customer"),
keeping the detail only in the plan's goal prose. Live repro: a `customers`
build produced a bare table (no `email`/`name`) and no route.

- [x] `plan_goal(plan_content)` — the plan's prose lines (not steps/headers).
- [x] `scaffold_first(project_dir, ctx, goal, files)` — resource/model detection
      still from the step; **fields fall back to the goal** when the step has none.
- [x] Same fallback for CRUD intent: a step about routes is promoted when the
      goal says CRUD (guarded on "route" so unrelated steps never scaffold).
- [x] Harden `detect_fields` against plan prose — `FIELD_REJECT_WORDS`
      (generate/create/follow/steps/routes/…), a ≤3-word cap, and an explicit
      `name:TYPE` branch that only fires for a REAL type (so "follow these
      steps:" can't become a column).
- [x] Wired all 3 call sites (`code` action passes `""`; both plan paths pass the goal).

### Verification (live, plan whose STEPS carry no fields and no "crud")
- [x] 113 Rust tests pass, clippy clean.
- [x] `POST /execute` → step 1 "4 files scaffolded", step 2 "2 files scaffolded"
      (was 0 — no route), **10 tests passed**, migrated + reloaded.
- [x] `customer` table has `email` + `name` — sourced from the GOAL line only.
- [x] `GET /api/customers` **404 → 200**, returns
      `{"records":[{"id":1,"email":"ada@example.com","name":"Ada",...}]}`.
- [x] No phantom resources, no bogus `follow_these_steps` column.

Note: during the first live run `tina4_chat` returned a transient "under
maintenance" message and the agent correctly SKIPPED those custom-code steps
rather than crashing. External service state, recovered on its own.

## Status: ✅ Complete — fields + CRUD intent survive the planner's rewrite.
