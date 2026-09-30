Research note, 2026-09-29, for review/REVIEW.md; not authority.

Topic: state restore and persistence scope, for D12, D35, §4.5 "Persistence",
the O2 restart scenarios and a proposed catalog/privacy rule. Systems: Android
(View, saved state, Compose), UIKit/SwiftUI, HTML + Blink + Firefox session
restore, VS Code hot exit, Redux Persist, TanStack Query persistence, Room,
Core Data, Automerge + Ink & Switch (local-first, Cambria), browser quotas,
Flutter restoration. Sibling note `query_command_effect_systems.md` covers
commands in flight at restart (outbox, idempotency); not repeated here.

Method: curl into `scratchpad/research/state_restore/`, then grep of the
fetched page or source file (AOSP, androidx, Chromium, Firefox, VS Code and
redux-persist sources read at their current `main`). Claims are marked
[verified: URL, what was read] or [not verified: reason].

## Summary

1. No default-capture system persists passwords (Blink, Firefox, SwiftUI warning); D35 has no sensitivity rule.
2. Android is the exception: `EditText` saves its text, passwords included, unless the app sets `saveEnabled=false`.
3. Saved-state lifetime follows user intent: force-quit or window close clears it, and a crash at launch discards it.
4. Positional keys (Compose, XPath, view ids) serve one build; cross-version stores use explicit renames plus migrations.
5. Under D12 a move or rename deletes data unless DRAIN pairs are written; the rewrite plan never says so.
6. Restore races are documented (TanStack mount vs restore; redux-persist timeout or migration error overwrites the store).
7. Inference: a re-run live query's first answer after restore is an update (D32) and resets piped HOLDs (D33).
8. Every host bounds restoration size (Android 1 MB per process, Firefox 2 M chars per field, 5 MiB localStorage).
9. Browsers evict a whole origin, never part of one; Boon should fail the whole store, never drop single leaves.
10. §4.5 still has a view-reachability rule that D35 abolished: an internal contradiction.

## 1. Android: View state, saved state, Compose

**Mechanism.** There are three tiers. ViewModel lives in memory and survives
configuration changes. Saved state is a Bundle that survives system-initiated
process death. Disk storage survives everything
[verified: https://developer.android.com/topic/libraries/architecture/saving-states,
table "Options for preserving UI state"].

**Evidence.**
- User dismissal clears state on purpose. Swipe from Recents, force-stop,
  reboot and `finish()` all remove "any saved state record", because users
  "expect the screen to start from a clean state" [verified: same page,
  "User-initiated UI state dismissal"].
- Saved state is written only when the Activity stops. Later writes wait for
  the next stop [verified: same page, "Important" note]. So a crash before a
  stop loses them.
- The Binder buffer is "currently 1MB, which is shared by all transactions in
  progress for the process". The docs advise keeping saved state "to less than
  50KB". Since API 24, going over throws `TransactionTooLargeException`
  [verified: https://developer.android.com/guide/components/activities/parcelables-and-bundles].
- The docs advise storing "an ID" rather than the data, and say saved state is
  "not a replacement for local storage" [verified: saving-states page].
- View state is keyed by view id [verified: AOSP `View.java`
  `dispatchSaveInstanceState`: `if (mID != NO_ID && (mViewFlags &
  SAVE_DISABLED_MASK) == 0) ... container.put(mID, state)`]. A view without
  an id saves nothing. Duplicate ids share one slot.
- `EditText.getFreezesText()` returns `true`. `TextView.onSaveInstanceState`
  saves `mText` (plus selection and `frozenWithFocus`) with no password check
  [verified: https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/main/core/java/android/widget/EditText.java
  and TextView.java]. So the brief's premise that a `textPassword` EditText
  is not restored by default is **false by the source**. The opt-out is
  `setSaveEnabled(false)`, which "can only disable the saving of this view"
  [verified: View.java javadoc].
- Compose `rememberSaveable` keys state by `currentCompositeKeyHashCode`, "the
  location in the composition tree". The explicit-`key` overload is now
  `@Deprecated`, because it "bypasses positional scoping, leading to state
  bugs". Restoration "DOES NOT validate against inputs provided before value
  was saved" [verified:
  https://raw.githubusercontent.com/androidx/androidx/androidx-main/compose/runtime/runtime-saveable/src/commonMain/kotlin/androidx/compose/runtime/saveable/RememberSaveable.kt].
  [not verified: whether composite keys stay stable across app versions.
  Saved state is only re-read by the same install, so the question does not
  arise for Compose.]

## 2. UIKit and SwiftUI

**Mechanism.** UIKit is opt-in per view controller through
`restorationIdentifier`. An archive holds the restoration paths. SwiftUI's
`@SceneStorage` is per-scene key/value state.

**Evidence.**
- UIKit "preserves the state of your app's views and view controllers to an
  encrypted file on disk". UIKit "can discard state preservation data at its
  discretion". The sample saves the app's version in the archive and refuses
  restoration if it does not match. A temporary login screen should get no
  restoration identifier [verified:
  https://developer.apple.com/tutorials/data/documentation/uikit/preserving-your-app-s-ui-across-launches.json].
- Restoration paths must be unique, and a parent without an identifier
  excludes its whole subtree. Guidance: "you would not preserve the view
  controller that asks for the new password information" [verified:
  https://developer.apple.com/library/archive/featuredarticles/ViewControllerPGforiPhoneOS/PreservingandRestoringState.html].
- Restoration runs "during the middle of your app's initialization". The
  restoration class may refuse a controller whose archive "refers to stale or
  missing data" [verified: .../uikit/about-the-ui-restoration-process.json].
- "The system automatically deletes an app's preserved state when the user
  force quits the app ... (The system also deletes preserved state if the app
  crashes at launch time as a similar safety precaution.)" [verified:
  https://developer.apple.com/library/archive/samplecode/StateRestoreChildViews/Listings/ReadMe_txt.html].
- `SceneStorage`: "system makes no guarantees as to when and how often the
  data will be persisted". Data must be "lightweight", not "model data", and
  it is destroyed when the scene is destroyed (window closed). "Do not use
  SceneStorage with sensitive data" [verified:
  https://developer.apple.com/tutorials/data/documentation/swiftui/scenestorage.json].
- [not verified: how a `secureTextEntry` UITextField behaves under state
  restoration. No Apple page found. The guide above only shows apps encoding
  field text themselves.]

## 3. Browsers: HTML autofill, Blink, Firefox session restore

**Mechanism.** Browsers save form-control state for history navigation and
session restore. Authors get one opt-out marker on the host element,
`autocomplete="off"`, rather than a language keyword.

**Evidence.**
- HTML Standard: for `off`, "the user agent should not remember the control's
  data", and values "are reset when reactivating a document" [verified:
  https://html.spec.whatwg.org/multipage/form-control-infrastructure.html,
  autofill expectation mantle].
- Blink: `PasswordInputType::ShouldSaveAndRestoreFormControlState()` returns
  `false`, and the save path is `NOTREACHED()` ("Should never save/restore
  password fields"). `autocomplete=off` on the element or its form also
  disables saving [verified: chromium `password_input_type.cc`,
  `html_form_control_element_with_state.cc`, main].
- Firefox `CollectInputElement` skips password, hidden, button, image, submit
  and reset inputs. It also skips inputs with autocomplete disabled or
  `!mCanAutomaticallyPersist`, values that pass `IsValidCCNumber`,
  `HasBeenTypePassword()` (a field that was ever a password), and values equal
  to the default `value` attribute ("only save data for form fields that have
  been changed"). The key is the element id, else an XPath, and
  `kMaxTraversedXPaths = 100` [verified:
  https://raw.githubusercontent.com/mozilla-firefox/firefox/main/toolkit/components/sessionstore/SessionStoreUtils.cpp].
- Firefox limits: `dom_form_limit` 2 M chars per field (a larger entry is
  silently skipped by `AppendEntry`), `dom_form_max_limit` 50 M per origin,
  `dom_storage_limit` 2048 and `interval` 15000 ms [verified: StaticPrefList.yaml].
  `privacy_level` means "0 = everywhere, 1 = unencrypted sites, 2 = nowhere".
  `max_resumed_crashes` is 1: after repeated crashes Firefox shows
  about:sessionrestore instead of restoring automatically [verified:
  browser/app/profile/firefox.js].

## 4. VS Code hot exit

**Mechanism.** Every dirty or untitled editor is backed up to disk, and the
backups are restored at the next start.

**Evidence.**
- Hot exit is on by default. `files.hotExit` takes `off`, `onExit` or
  `onExitAndWindowClose`. Backups live in `~/.config/Code/Backups` on Linux
  [verified: https://code.visualstudio.com/docs/editing/codebasics, "Hot Exit"].
- Backups are written "periodically", so a crash restores them too. Hot exit
  was limited to closing all windows "due to this fear of having backups
  persisted but not be discoverable" [verified:
  https://raw.githubusercontent.com/microsoft/vscode-docs/main/blogs/2016/11/30/hot-exit-in-insiders.md].
- Key: `hashIdentifier` hashes the file's `fsPath` (plus a type id when there
  is one). "For backwards compatibility ... we ignore the typeId unless a
  value is provided". The old hash scheme was frozen so old backups still
  resolve. A backup file is a preamble (resource URI + JSON meta, max 10000
  chars) followed by the raw content, written through `fileService.writeFile`
  with no encryption step [verified:
  https://raw.githubusercontent.com/microsoft/vscode/main/src/vs/workbench/services/workingCopy/common/workingCopyBackupService.ts].
  So an unsaved `.env` lands in plain text in the user profile.
- Identity reuse: "consider the case of a folder getting deleted and recreated
  with the same name". The workspace gets a ctime identifier, and backups are
  discarded on mismatch [verified: https://api.github.com/repos/microsoft/vscode/issues/14054].
- [not verified: what happens to a dirty file's backup when the file is
  renamed on disk while VS Code is closed.]

## 5. Redux Persist

**Mechanism.** The whole store is serialized under a `key`. Top-level reducer
keys are filtered by `whitelist`/`blacklist` (one level deep; nest a
persistReducer to go deeper). Old state goes through `version` + `migrate`
before being reconciled [verified:
https://raw.githubusercontent.com/rt2zz/redux-persist/master/README.md].

**Evidence (source, master).**
- `createMigrate` runs the integer-keyed migrations above the stored version
  in order. On a downgrade it logs "downgrading version is not supported" and
  **returns the newer state unchanged** [verified: src/createMigrate.ts].
- `autoMergeLevel1` hard-sets every inbound top-level key, including keys the
  current reducers no longer have. It skips a key the reducer already changed
  before REHYDRATE. So a renamed reducer key starts from its default, and the
  old key is carried along as junk [verified: src/stateReconciler/autoMergeLevel1.ts].
- `persistReducer` has a default `timeout` of 5000 ms. On timeout, on a
  storage error or on a migration error it calls `_rehydrate(undefined, err)`.
  The state is marked `rehydrated: true`, and `conditionalUpdate` then writes
  the current (default) state to storage. The stored state is overwritten
  [verified: src/persistReducer.ts].
- `PersistGate` "delays the rendering of your app's UI until your persisted
  state has been retrieved" [verified: README].

## 6. TanStack Query persistence

**Evidence** [verified: https://raw.githubusercontent.com/TanStack/query/main/docs/framework/react/plugins/persistQueryClient.md]:
- Query results are persisted as a cache. A cache is discarded when it is
  expired (`maxAge`, 24 h by default), "busted" (a `buster` string such as a
  build hash does not match), errored or empty.
- Documented race: "if you render your App while you are restoring, you might
  get into race conditions if a query mounts and fetches at the same time".
  `PersistQueryClientProvider` holds queries in `fetchingState: 'idle'` until
  the restore finishes, then refetches unless the data is fresh.
  `useIsRestoring` exposes the phase. `onSuccess` is where paused mutations
  are resumed.

## 7. Room and Core Data (schema versioning)

- Room auto-migrations diff the schema JSON exported at compile time. "If Room
  detects ambiguous schema changes ... it throws a compile-time error". Deleting
  or renaming a table or column needs an `AutoMigrationSpec` with
  `@RenameColumn`, `@DeleteColumn` and similar. A missing path is an
  `IllegalStateException`, unless `fallbackToDestructiveMigration` is set, which
  "permanently deletes all data". Narrower variants exist: `...From`,
  `...OnDowngrade` [verified:
  https://developer.android.com/training/data-storage/room/migrating-db-versions].
- Core Data infers lightweight mappings for obvious changes (add an attribute,
  make one optional, make one non-optional with a default). A rename uses a
  **renaming identifier** that "creates a 'canonical name'", so v1→v3 works
  after two renames. Source and destination models must both be findable at
  runtime [verified:
  https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/CoreDataVersioning/Articles/vmLightweightMigration.html].

## 8. Automerge, Ink & Switch local-first and Cambria

- Automerge: "the easiest way" to set up the schema is an initial `change()`
  synced to every device. Devices that each create their own schema change
  "will not work". The recommended fix is hard-coded, deterministic migration
  changes applied identically everywhere, one per version ("Do not modify the
  initial change"). Old app versions must keep working. Cambria "is not yet
  implemented in Automerge" [verified: https://automerge.org/docs/cookbook/modeling-data/].
- Local-first essay: with no central server "there is no authoritative
  'current' schema", which it lists as open. Its advice is to "make clear to
  users when the data is stored only on their device" [verified:
  https://www.inkandswitch.com/essay/local-first/].
- Cambria: a naive rename of `authors` to `contributors` loses data when old
  versions "reintroduce the authors field". Its answer is bidirectional lenses
  in a graph, stored "where even old versions of the program can retrieve
  them" [verified: https://www.inkandswitch.com/cambria/].

## 9. Quotas and eviction; Flutter restoration

- localStorage and sessionStorage: 5 MiB each per origin, then
  `QuotaExceededError`. IndexedDB best-effort: Firefox min(10 % of disk, 10 GiB
  group), Chromium 60 %. Safari deletes script-written data after 7 days
  without interaction. Eviction removes all of an origin's data, because
  "Only deleting some of the origin's data could cause inconsistency problems"
  [verified: https://developer.mozilla.org/en-US/docs/Web/API/Storage_API/Storage_quotas_and_eviction_criteria].
- Flutter: restoration buckets are keyed by restoration IDs in a tree. Data
  "should be as small as possible", and data that can be fetched elsewhere is
  replaced by an identifier. The tree is pushed to the engine at the end of
  every frame that changed it. A running app can receive a new root bucket
  (browser back/forward), and listeners must then re-restore [verified:
  https://api.flutter.dev/flutter/services/RestorationManager-class.html].

## Answers to the four questions

1. **Persist-by-default and secrets.** Closest to D35: Firefox restore,
   Blink form state, VS Code hot exit, Android id'd views. Browsers
   hard-exclude passwords, honour `autocomplete=off` and (Firefox) add
   heuristics. Android and VS Code exclude nothing (Bundle in memory; plain
   text on disk). Apple is opt-in, encrypts, and warns against secrets.
2. **Rename/move.** Positional or id keys lose data silently (§1, §3, §5).
   Versioned stores need a declared rename (§7, §8). Minimum facility: a rename
   marker, pure ordered migration steps, a diff against the previous exported
   schema that fails on ambiguity, fail-closed with opt-in reset, tombstones.
3. **Ordering.** No output before restore matches PersistGate, TanStack
   `isRestoring` and UIKit. Queries re-run after restore as in TanStack.
   Documented races: fetch vs restore, input before rehydrate, and
   timeout/error falling back to defaults that then overwrite storage.
4. **Size.** Every host bounds restoration data; browser stores fail with
   `QuotaExceededError` or evict a whole origin. Firefox's per-field dropping
   suits independent fields, not interdependent Boon leaves.

## Implications for the plan

1. **Add a sensitivity rule to D35 (critical).** The plan says "All app memory
   persists whenever the deployment has a store", and D12 adds "the last value
   of an app-computed value that code reads outside its own update". Every
   default-capture browser hard-excludes passwords (§3). The rewrite plan
   never mentions secrets, and D12/D35 do not reference the older plan's
   "Sensitive Input And Credentials" section. Leak route today:
   `examples/host_service_effects.bn` reads `verify_secret.secret`. Any
   derived field over that payload that is read outside its update becomes a
   durable leaf. The R9 effect log is another sink.
   *Proposed:* catalog ports and payload fields carry a `sensitive` class
   (password text, one-time codes, `Secret/*` and `Crypto/*` key inputs). The
   checker propagates it. A durable leaf (HOLD, LATEST, row field, last-value
   leaf) that may hold a sensitive value is a positioned error. Password
   element text stays a host reference and never enters Boon memory (as the
   old plan says). Redaction covers the effect log and the dev inspector.
   Add an O2 scenario: type a password, restart, check the store bytes and
   the empty field.
2. **Fix the §4.5 contradiction (high).** §4.5 says "State reachable only from
   view code, such as element-local hover, is transient (L13)". D35 says "no
   view/model split and no reachability rule", and L13 is answered by D35.
   *Proposed:* replace the bullet with "host-known values are host SOURCEs
   and are never durable (D35); every other memory is durable". Prior art has
   lifetime tiers (Android, §1), not reachability rules.
3. **State that a move or rename changes identity, and name the migration
   facility (high, D12, §4.5).** Evidence: §1, §5 and §7. D35's "moving a HOLD
   must not change whether it survives" is about eligibility, but under D12 a
   move changes the structural route. The old plan says a move without a DRAIN
   pair deletes the old authority. The rewrite plan names DRAIN only inside
   identity_v1 and `persistence_only` stages.
   *Proposed:* D12 says explicitly that DRAIN/DRAINING and the sequential
   catalog are kept. Add a Room-style check against the previous schema: when
   a durable leaf disappears and a same-typed leaf appears under the same
   owner, emit a positioned diagnostic with fix-its "add `DRAIN { old }`" or
   "confirm deletion". Spike: apply extract-function, wrap-in-BLOCK, reorder
   and rename edits to the migration examples, and count identity changes per
   refactor.
4. **Record deletions as tombstones in the catalog (medium).** Evidence: VS Code
   #14054 (a path reused after deletion), Cambria (old versions reintroduce a
   field). A store that skips v2 (delete `count`) and opens v3 (a new `count`
   with the same type) gets the v1 value back. The old plan's "Sequential
   Migration Catalog" lists only DRAIN edges.
   *Proposed:* deletions become catalog edges, or MemoryId gets an incarnation
   number bumped on re-creation. Add a golden vector for it.
5. **A re-run query resets restored HOLDs (high, D12 + D31/D32/D33,
   inference).** D32 says effect results fire on every completion, and D33
   says "a later update of the piped value resets the HOLD". So `query |> HOLD
   draft {…}` restores `draft` and then loses it when the re-run query
   answers, on every restart. A LATEST with a live-query arm has the same
   problem. Compose restores without checking inputs; TanStack does not
   refetch when the restored data is fresh.
   *Proposed:* P0 decides whether the first answer after restore is an update.
   Options: persist the last query answer and treat the re-run as a refresh;
   or mark the first post-restore answer as "not an update"; or accept the
   reset and document it. Add an O2 scenario either way.
6. **Pin the meaning of "re-run on restore" and the first frame (high, D12,
   D34, L3c).** D12 says "Query results are re-run on restore". A copy-context
   query result that landed in a HOLD is HOLD state, so it is restored, not
   re-run, and its copied arguments (for example a `Secret/verify` secret)
   are never stored. Live queries have no last answer at a cold start, so
   they render nothing (L3c) until they answer. The old plan's "Restore
   completes before any observable output is published" does not say whether
   it waits for queries.
   *Proposed:* write both points into §4.5. Choose "publish without query
   values" or "wait for queries up to a bound", and assert the first frame in
   O2. A TanStack-style non-authoritative query cache (max age + build-hash
   buster) is later work.
7. **Never fall back to defaults over a stored state (high, §4.5, O2).** Evidence:
   §5 (redux-persist timeout and migration error), §7 (Room fails closed). The
   old plan already says "keep the old database untouched when migration
   cannot start safely".
   *Proposed:* make it named O2 negative scenarios: migration throws, the
   store is slow or corrupt. Check that the store bytes are unchanged and the
   app runs in memory with a visible "not durable" status. Also define what
   happens to host occurrences that arrive before restore settles: queue or
   drop.
8. **Add a crash-loop guard (high, §4.5, O2).** Evidence: UIKit deletes
   preserved state after a crash at launch; Firefox `max_resumed_crashes=1`.
   Under D35 a restored state that panics or overruns a budget in the first
   settle crashes the app on every start.
   *Proposed:* the host counts consecutive failed starts (restore + first
   settle + first publish). After N it offers "start fresh" and moves the old
   store aside as a backup, never deleting it. Add an O2 scenario that injects
   a failure in the first settle.
9. **Define the lifetime and the "restored" occurrence (medium, D35).** Evidence:
   §1-§4 clear state on user intent. Flutter can restore a running app. D35
   gives one lifetime and leaves resets to app logic on "restored".
   *Proposed:* define exactly when "restored" fires: cold start with restored
   memory, crash recovery, upgrade migration, browser back/forward; not on
   hot reload. Give it a reason tag. Add a product-level "Reset app state"
   host action (the old plan has only the dev Clear State).
10. **Give each host port a restart class in the catalog (medium, D35, §5.1).**
    Android restores `frozenWithFocus` and selection (§1), and UIKit restores
    the first responder. TodoMVC's focus moves from a HOLD, which would
    persist, to a host `focused` port, so focus after a restart now depends
    on the host.
    *Proposed:* each level port declares `resupplied` (pointer, hover,
    connections, time), `host-restored` (focus, scroll, selection, window
    geometry; the host saves these in its own store keyed by R4 element
    identity) or `reset` (pressed). O2 asserts focus and scroll after restart.
11. **Add a durable-size budget with whole-store semantics (medium, D35, §4.5).**
    Evidence: §1, §3, §9. D35 makes all memory durable with no bound.
    *Proposed:* the dev window shows a static size class per leaf (bounded:
    Bool, Number, tag, `BYTES[n]`; unbounded: TEXT, BYTES, collections). The
    deployment sets a budget. When the budget is exceeded, the app keeps
    running in memory and reports it. It never drops single leaves. Lift the
    old plan's quota and blob rules into §4.5. O2 covers the quota case.
12. **Offer at-rest protection as a deployment option (low, D35).** UIKit
    encrypts its archive; VS Code writes plain text; Firefox has
    `privacy_level`. A deployment option (platform data protection or
    encryption, and a "do not persist text drafts" policy) keeps D35's "the
    deployment picks the store" without a keyword.

## Sources

All fetched with curl on 2026-09-29/30 unless noted.
1. developer.android.com: topic/libraries/architecture/saving-states; guide/components/activities/parcelables-and-bundles; training/data-storage/room/migrating-db-versions; reference/kotlin/androidx/compose/runtime/saveable/package-summary (no key semantics there).
2. AOSP `View.java`, `TextView.java`, `EditText.java` (raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/main); androidx `RememberSaveable.kt` (androidx-main).
3. developer.apple.com/tutorials/data/documentation: uikit/preserving-your-app-s-ui-across-launches.json, uikit/about-the-ui-restoration-process.json, uikit/uiview/restorationidentifier.json, swiftui/scenestorage.json.
4. developer.apple.com/library/archive: featuredarticles/ViewControllerPGforiPhoneOS/PreservingandRestoringState.html; samplecode/StateRestoreChildViews/Listings/ReadMe_txt.html; documentation/Cocoa/Conceptual/CoreDataVersioning/Articles/vmLightweightMigration.html (the modern JSON page returned 404).
5. https://html.spec.whatwg.org/multipage/form-control-infrastructure.html
6. Chromium `password_input_type.cc`, `html_input_element.cc`, `html_form_control_element_with_state.cc` (main).
7. Firefox `SessionStoreUtils.cpp`, `StaticPrefList.yaml`, `firefox.js` (raw.githubusercontent.com/mozilla-firefox/firefox/main).
8. https://code.visualstudio.com/docs/editing/codebasics; vscode-docs `blogs/2016/11/30/hot-exit-in-insiders.md`; vscode `workingCopyBackupService.ts` (main); api.github.com microsoft/vscode issue 14054.
9. redux-persist (master): `README.md`, `docs/migrations.md`, `src/createMigrate.ts`, `src/stateReconciler/autoMergeLevel1.ts`, `src/persistReducer.ts`.
10. TanStack Query `docs/framework/react/plugins/persistQueryClient.md` (main).
11. https://automerge.org/docs/cookbook/modeling-data/; https://www.inkandswitch.com/essay/local-first/; https://www.inkandswitch.com/cambria/
12. https://developer.mozilla.org/en-US/docs/Web/API/Storage_API/Storage_quotas_and_eviction_criteria
13. https://api.flutter.dev/flutter/services/RestorationManager-class.html
14. Not covered: Yjs (budget); iOS `secureTextEntry` restore (no primary source found).
