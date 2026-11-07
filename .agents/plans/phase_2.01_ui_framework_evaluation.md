# Phase 2.01: UI Framework Evaluation — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.5), §12
**Status:** Draft — Spike
**Duration:** 1 week
**Depends on:** Phase 2 completion (confirmed 2026-10-23)

---

## Purpose

Phase 2.1 was paused because the initial GPUI implementation revealed friction. GPUI's pre-1.0 API surface and tightly-coupled test harness present a long-term risk for a multi-phase project. This spike evaluates GPUI against three alternatives — Iced, egui, and Fenestra — using a consistent, hands-on methodology.

The goal is to make an informed decision on the UI framework **before** committing to the construction of the application shell.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Building a non-trivial UI prototype (Unlock Screen) in each framework | Building the full application shell |
| Evaluating headless testing capabilities (automation, snapshot) | Evaluating visual polish or theming |
| Assessing API stability and developer experience | Performance benchmarking with large datasets |
| Measuring compile time and binary size | Production implementation of any screen |

---

## Methodology: The Common Prototype

For each framework, implement the same **Unlock Screen** prototype. This screen is chosen because it exercises essential patterns: a form (passphrase input), a button action, an error state, and a conditional flow (switching to recovery).

The prototype must include:

1. A passphrase input field.
2. An "Unlock" button that triggers an async operation.
3. A display area for an error message on failure.
4. A "Switch to Recovery" button that navigates to a placeholder screen.

---

## Evaluation Criteria

Each framework will be assessed against the following criteria, with a clear winner identified for each category.

### 1. Testability (Primary)

- **Headless automation:** Can the prototype be driven headlessly to simulate clicks and text input?
- **Semantic queries:** Can elements be found by role and accessible name (e.g., "find the button named 'Unlock'") rather than by coordinates?
- **Snapshot testing:** Can the rendered output be captured as an image and compared against a golden file for visual regressions?
- **Ecosystem maturity:** Is the testing framework a first-class, documented part of the ecosystem?

### 2. Performance (Primary)

- **Rendering model:** Is it immediate-mode (re-renders every frame) or retained/declarative (only updates changes)? What are the implications for a data-dense application?
- **Idle CPU usage:** Does the framework consume CPU when the UI is static?
- **Binary size:** What is the final binary size of the prototype?

### 3. Developer Experience & Stability (Secondary)

- **API stability:** Is the framework pre-1.0? How frequently does the API change?
- **Compile time:** How long does a clean build of the prototype take?
- **Documentation:** Is there a clear getting-started guide? Are there complete examples?
- **Ecosystem:** Are there pre-built widgets, or is everything built from scratch?

---

## Spike Execution Steps

### Step 1: Fenestra Prototype (Days 1–2)

Fenestra is evaluated first because its stated thesis is testability.

- **Task 1.1:** Create a new project from the Fenestra template.
- **Task 1.2:** Implement the Unlock Screen prototype using Fenestra's application structure.
- **Task 1.3:** Write a headless test for the prototype. The test should:
    - Find the input by its accessible name and type a passphrase.
    - Find the button by its role and name ("Unlock") and click it.
    - Assert that the correct state transition occurred.
    - Capture a PNG snapshot and compare it against a golden file.
- **Task 1.4:** Measure idle CPU usage and the final binary size.

### Step 2: Iced Prototype (Days 3–4)

Iced is the most mature alternative with a strong testing story.

- **Task 2.1:** Create a new Iced project.
- **Task 2.2:** Implement the Unlock Screen prototype using the Elm architecture.
- **Task 2.3:** Write a headless test using the `iced_test` crate. The test should:
    - Create a `Simulator` for the UI.
    - Use `tap_key` and `typewrite` to enter a passphrase.
    - Simulate a click on the "Unlock" button.
    - Assert that the application state changed correctly.
    - Use the simulator's snapshot feature to capture the UI state.
- **Task 2.4:** Measure idle CPU usage and the final binary size.

### Step 3: egui Prototype (Day 5)

egui is evaluated for its mature testing ecosystem and immediate-mode simplicity.

- **Task 3.1:** Create a new `eframe` project.
- **Task 3.2:** Implement the Unlock Screen prototype using egui's immediate-mode API.
- **Task 3.3:** Write a headless test using `egui_kittest`. The test should:
    - Use the `Harness` to drive the UI.
    - Use `get_by_role` and `get_by_name` to find and interact with elements.
    - Use `Harness::snapshot` to capture a screenshot for visual regression testing.
- **Task 3.4:** Measure idle CPU usage and the final binary size.

### Step 4: GPUI Prototype (Reference)

The existing GPUI implementation is used as a baseline. The goal is to document the friction encountered and measure its performance characteristics.

- **Task 4.1:** Complete the minimal GPUI window and attempt to implement the Unlock Screen.
- **Task 4.2:** Investigate GPUI's test support (`TestAppContext`). Document how it compares to the other frameworks' testing solutions in terms of ease of use and documentation.
- **Task 4.3:** Measure idle CPU usage and binary size.

### Step 5: Synthesis and Decision (End of Week)

- **Task 5.1:** Consolidate all findings into a comparison matrix in `docs/ui-framework-spike.md`.
- **Task 5.2:** Write a recommendation, justifying the choice based on the evaluation criteria, with **testability and performance** weighted highest.
- **Task 5.3:** Update the main plan and the Phase 2.1 plan to reflect the final decision.

---

## Decision Matrix Template

| Criterion | Fenestra | Iced | egui | GPUI |
| :--- | :--- | :--- | :--- | :--- |
| **Headless automation** | TBD | Yes (`Simulator`) | Yes (`kittest`) | Limited, coupled to Zed |
| **Semantic queries** | TBD | No (by widget) | Yes (`get_by_role`) | Unverified |
| **Snapshot testing** | TBD | Yes (`snapshot`) | Yes (`snapshot`) | Unverified |
| **Rendering model** | TBD | Retained (Elm) | Immediate-mode | Hybrid |
| **API stability** | TBD | Pre-1.0 | Mature | Pre-1.0 |
| **Idle CPU** | TBD | Very low (reactive) | Moderate (re-paints) | Low |
| **Binary size** | TBD | TBD | TBD | TBD |
| **Ecosystem** | TBD | Mature (built-in widgets) | Mature (many add-on crates) | Young |
| **Recommendation** | | | | |

---

## Exit Criteria

This spike is complete when:

1. The Unlock Screen prototype has been implemented in Fenestra, Iced, and egui.
2. A headless test has been written for each of the three prototypes, covering automation and snapshot testing.
3. The comparison matrix is filled out with measured data (idle CPU, binary size) and qualitative findings.
4. A final recommendation is documented in `docs/ui-framework-spike.md`.
5. The main plan and Phase 2.1 plan are updated to reflect the chosen framework.

---

## What This Spike Is Not

This is not a full performance benchmark or a complete implementation of the application. It is a targeted, time-boxed evaluation designed to answer one question with evidence, not opinion. Once the decision is made, the prototype code is discarded, and Phase 2.1 begins on the chosen foundation.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2.01-R1 | **Fenestra is too young to evaluate.** The project may not have a usable template, testing harness, or documentation at the time of the spike. | Medium | Timebox Fenestra to two days. If it cannot be evaluated in that window, record it as "not yet viable" and proceed with the other three. |
| P2.01-R2 | **Prototype scope creep.** The Unlock Screen prototype grows into a partial shell. | Medium | The prototype is four elements. Anything beyond that is out of scope. |
| P2.01-R3 | **Results are subjective.** Developer experience is hard to measure. | Medium | Weight testability and performance highest — both are measurable. DX is a tiebreaker, not a decider. |
| P2.01-R4 | **The chosen framework changes before Phase 2.1 completes.** | Medium | Every framework here is pre- or post-1.0 but moving. The spike should record the version tested and pin it. |
| P2.01-R5 | **Sunk-cost bias.** Having built Phase 1 and 2 against GPUI, there is pressure to conclude GPUI was right. | High | The decision criteria are written before any prototype is built. The GPUI column is evaluated last, as a reference, not as a candidate. |