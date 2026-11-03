#!/usr/bin/env node
// Build a demo vault for Vault HQ: a fictional person's notes, a week of daily notes,
// inbox captures and their own Taskwarrior data, all dated relative to today.
// Nothing here touches a real vault or your own tasks: Taskwarrior runs with its own
// data folder and hooks off, so no sync hook can fire.
//
//   node demo/make-demo.mjs [folder]        default /Users/Shared/hq-demo
//   then:  npm run demo                     serves it on http://localhost:4330
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(process.argv[2] ?? "/Users/Shared/hq-demo");
const vault = path.join(root, "demo-vault");
const state = path.join(root, "state");
const taskData = path.join(root, "taskwarrior");
const taskrc = path.join(root, "taskrc");
const taskBin = path.join(root, "task");
const TASK = ["/opt/homebrew/bin/task", "/usr/local/bin/task"].find((p) => fs.existsSync(p));

const iso = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
const day = (n) => {
  const d = new Date();
  d.setDate(d.getDate() + n);
  return iso(d);
};
const T = day(0);

fs.rmSync(root, { recursive: true, force: true });
const write = (rel, text) => {
  const f = path.join(vault, rel);
  fs.mkdirSync(path.dirname(f), { recursive: true });
  fs.writeFileSync(f, text.replace(/^\n/, ""));
};
const fm = (props) => `---\n${Object.entries(props).map(([k, v]) => `${k}: ${v}`).join("\n")}\n---\n\n`;

fs.mkdirSync(path.join(vault, ".obsidian"), { recursive: true });

write("AGENTS.md", `${fm({ title: "Agent contract", type: "index", created: day(-40), updated: day(-3), author: "human" })}# Agent contract

Every agent and editor writes into this vault the same way:

- New things land in **00-Inbox** first; nothing is filed without a human look.
- Projects live in **01-Projects**, one note each, with a **Progress:** line and open tasks as checkboxes.
- Daily notes (**09-Planning/Daily**) have human sections (Quick Wins, Work, Learning, Tomorrow) and an **Agent log** that only agents append to.
- \`author: human\` notes are edited by the human; agents add proposals beside them instead.
`);

write("01-Projects/garden-irrigation.md", `${fm({ title: "Garden drip irrigation", type: "project", status: "active", created: day(-30), updated: day(-1), author: "human", tags: "[project, garden]" })}# Garden drip irrigation

**Status:** active · **Progress:** 60%

## Description
Replace hand watering of the four raised beds with a drip line on a timer, so the
garden survives a week away. Zones follow the sun map in [[garden-zone-map]].

## Tasks
- [x] Measure beds and draw the zone map ✅ ${day(-20)}
- [x] Order 1/2" main line, emitters and a two-zone timer ✅ ${day(-12)}
- [ ] Lay the main line along the fence 🔼 📅 ${day(-2)}
- [ ] Test each zone for 10 minutes and note dry spots 📅 ${day(1)}
- [ ] Add a rain sensor before winter 🔽

## Log
- ${day(-12)}: parts ordered; the timer takes two AA batteries, no Wi-Fi needed.
- ${day(-1)}: bed 3 gets afternoon shade, so it can share a zone with bed 4.
`);

write("01-Projects/learn-rust.md", `${fm({ title: "Learn Rust", type: "project", status: "active", created: day(-25), updated: day(-2), author: "human", tags: "[project, learning, rust]" })}# Learn Rust

**Status:** active · **Progress:** 35%

## Description
Work through the book to chapter 12, then write a small CLI that tidies the
Downloads folder into subfolders by file type. Notes go in [[rust-ownership]].

## Tasks
- [x] Chapters 1-6 ✅ ${day(-9)}
- [ ] Chapters 7-9: modules, collections, errors ⏫ 📅 ${day(2)}
- [ ] Sketch the tidy-downloads CLI: arguments, dry-run flag, undo log
- [ ] Read about \`Result\` and the \`?\` operator again 🔽
`);

write("01-Projects/home-network-refresh.md", `${fm({ title: "Home network refresh", type: "project", status: "active", created: day(-35), updated: day(-4), author: "human", tags: "[project, home]" })}# Home network refresh

**Status:** active · **Progress:** 80%

## Description
Swap the old router for a three-node mesh, put guests and smart plugs on their own
network, and write down how it all fits so the next fix takes minutes, not an evening.

## Tasks
- [x] Mesh nodes in the hall, office and garage ✅ ${day(-15)}
- [x] Guest network with its own password ✅ ${day(-14)}
- [ ] Move the smart plugs to the IoT network 📅 ${day(0)}
- [ ] Print the guest Wi-Fi card for the fridge

| Network | For | Notes |
|---|---|---|
| home | laptops, phones | WPA3 |
| guests | visitors | isolated, rotates monthly |
| iot | plugs, the printer | no access to home |
`);

write("01-Projects/book-club.md", `${fm({ title: "Neighbourhood book club", type: "project", status: "active", created: day(-60), updated: day(-21), author: "human", tags: "[project, community]" })}# Neighbourhood book club

**Status:** active · **Progress:** 20%

## Description
Six neighbours, one book a month, hosted in turn. Keep the reading list and who hosts
next in one place.

## Tasks
- [ ] Pick next month's book from the shortlist
- [ ] Ask who can host in December
`);

write("05-References/espresso-dial-in.md", `${fm({ title: "Espresso dial-in", type: "reference", created: day(-50), updated: day(-6), author: "human", tags: "[reference, coffee]" })}# Espresso dial-in

What works on this grinder and machine. Change one thing at a time.

| Beans | Dose | Yield | Time | Grind | Taste |
|---|---|---|---|---|---|
| House blend (medium) | 18 g | 36 g | 28 s | 12 | balanced, a little nutty |
| Ethiopian (light) | 18 g | 40 g | 30 s | 9 | bright; finer was sour |
| Decaf (dark) | 17 g | 34 g | 25 s | 14 | best at 1:2 |

> [!tip] Rule of thumb
> Sour and fast: grind finer. Bitter and slow: grind coarser.

See also [[home-network-refresh]] for the smart plug that warms the machine at 6:30.
`);

write("05-References/garden-zone-map.md", `${fm({ title: "Garden zone map", type: "reference", created: day(-20), updated: day(-1), author: "human", tags: "[reference, garden]" })}# Garden zone map

- **Zone 1** (full sun): beds 1 and 2, tomatoes and peppers. 20 minutes, early morning.
- **Zone 2** (afternoon shade): beds 3 and 4, greens and herbs. 10 minutes.

Back to [[garden-irrigation]].
`);

write("05-References/model-findings.md", `${fm({ title: "Local model findings", type: "findings", created: day(-14), updated: day(-2), author: "claude", tags: "[findings, ai]" })}# Local model findings

Which local model answers questions about this vault best. Same ten questions each run.

### ${day(-14)} llama3.1:8b
7/10 right; mixed up two project dates.

### ${day(-2)} qwen2.5:14b
9/10 right; slower, but cites the right note every time.
`);

write("03-Learning/rust-ownership.md", `${fm({ title: "Rust ownership, in my words", type: "learning", created: day(-9), updated: day(-2), author: "human", tags: "[learning, rust]" })}# Rust ownership, in my words

1. Every value has one owner.
2. You can lend it out: many readers (\`&T\`) **or** one writer (\`&mut T\`), never both.
3. When the owner goes out of scope, the value is dropped.

\`\`\`rust
fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() >= b.len() { a } else { b }
}
\`\`\`

Part of [[learn-rust]].
`);

write("06-Memory/preferences.md", `${fm({ title: "How I like to work", type: "memory", created: day(-40), updated: day(-10), author: "human", tags: "[memory]" })}# How I like to work

- Mornings for deep work, afternoons for errands.
- Short notes over long ones; link instead of repeating.
- Agents propose, I decide.
`);

write(`00-Inbox/${T}-capture.md`, `${fm({ title: "Quick captures", type: "capture", created: T, updated: T, author: "human" })}# Quick captures

- Try a 1:2.2 ratio with the new Ethiopian beans
- Book idea for the club: *The Overstory*
`);
write(`00-Inbox/${day(-1)}-session-garden.md`, `${fm({ title: "Session: planning the drip zones", type: "session", created: day(-1), updated: day(-1), author: "claude" })}# Session: planning the drip zones

The user and the agent split the four beds into two zones by afternoon shade.
Proposed: add the zone timings to [[garden-zone-map]] (done) and a test task to
[[garden-irrigation]] (done).
`);
write(`00-Inbox/${day(-2)}-proposal-rust-cli.md`, `${fm({ title: "Proposal: start the Rust CLI as a dry run only", type: "proposal", created: day(-2), updated: day(-2), author: "claude" })}# Proposal: start the Rust CLI as a dry run only

Build the Downloads tidier so its first version only prints what it would move. Add
real moves once the undo log exists. Affects [[learn-rust]].
`);

const humans = [
  { wins: ["Watered by hand one last time"], work: ["Drew the zone map"], learn: ["Chapter 6: enums and match"], tomorrow: ["Lay the main line"] },
  { wins: ["Printed the guest Wi-Fi card"], work: ["Mesh firmware updated"], learn: ["Chapter 7 started"], tomorrow: ["Book club shortlist"] },
  { wins: ["Dialled in the decaf"], work: ["Ordered emitters"], learn: [], tomorrow: ["Garden: test zone 1"] },
];
for (let i = -6; i <= 0; i++) {
  const d = day(i);
  const h = humans[(i + 6) % humans.length];
  const agents = [
    `09:1${(i + 9) % 10} claude@laptop · session (garden, 3 prompts): planned drip zones by afternoon shade`,
    `14:0${(i + 9) % 10} claude@laptop · filed [[garden-zone-map]] update from the inbox`,
    ...(i % 2 === 0 ? [`18:30 hq@laptop · decided: Lay the main line → Move to next week`] : []),
  ].slice(0, i === 0 ? 2 : 3);
  write(
    `09-Planning/Daily/${d}.md`,
    `${fm({
      title: d,
      type: "daily",
      created: d,
      updated: d,
      author: "human",
      tasks_pending: 14 - Math.max(0, i + 6),
      tasks_done_today: [1, 0, 2, 1, 3, 0, 2][i + 6],
      vault_inbox: [1, 2, 2, 3, 1, 2, 3][i + 6],
      metrics_updated: "21:05",
    })}# ${d}

## Quick Wins
${h.wins.map((x) => `- ${x}`).join("\n")}

## Work
${h.work.map((x) => `- ${x}`).join("\n")}

## Learning
${h.learn.map((x) => `- ${x}`).join("\n")}

## Tomorrow
${h.tomorrow.map((x) => `- ${x}`).join("\n")}

## Agent log
${agents.map((x) => `- ${x}`).join("\n")}
`,
  );
}

write("09-Planning/decision-log.md", `${fm({ title: "Decision log", type: "index", created: day(-30), updated: day(-2), author: "human" })}# Decision log

## ${day(-2)} 18:30: Lay the main line along the fence
- Kind: Overdue
- Outcome: Move to next week
- Why: Rain all weekend; the line can wait, the parts are in the garage.
`);

// Taskwarrior: its own data, hooks off, so nothing reaches a real vault
fs.mkdirSync(taskData, { recursive: true });
fs.writeFileSync(taskrc, `data.location=${taskData}\nhooks=0\nconfirmation=off\nverbose=nothing\nnews.version=99.0.0\n`);
fs.writeFileSync(taskBin, `#!/bin/sh\n# Taskwarrior for the Vault HQ demo only: own data, hooks off.\nexec env TASKRC="${taskrc}" TASKDATA="${taskData}" "${TASK ?? "task"}" "$@"\n`);
fs.chmodSync(taskBin, 0o755);
if (TASK) {
  const add = (...a) => execFileSync(taskBin, ["rc.hooks=0", "add", ...a], { stdio: "ignore" });
  add("Lay the main line along the fence", "project:garden-irrigation", "priority:M", `due:${day(-2)}`);
  add("Test each zone for 10 minutes", "project:garden-irrigation", `due:${day(1)}`);
  add("Chapters 7-9: modules, collections, errors", "project:learn-rust", "priority:H", `due:${day(2)}`);
  add("Move the smart plugs to the IoT network", "project:home-network-refresh", `due:${T}`);
  add("Pick next month's book from the shortlist", "project:book-club");
  add("Sketch the tidy-downloads CLI", "project:learn-rust", "priority:L");
}

fs.mkdirSync(state, { recursive: true });
fs.writeFileSync(path.join(state, "task-sync.json"), "{}"); // "synced just now" in the top bar
console.log(`demo vault: ${vault}\nstate:      ${state}\ntasks:      ${TASK ? taskBin : "Taskwarrior not installed; the Tasks view will be empty"}`);
