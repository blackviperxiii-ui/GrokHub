# Built-in vstack skills: credit and license

Bundled from [vstack](https://github.com/blackviperxiii-ui/vstack) release
[`v1.3.0`](https://github.com/blackviperxiii-ui/vstack/releases/tag/v1.3.0) (commit `c296c0f714d0934353f99879e6f8b00ffdc7f789`).
`.github/scripts/vstack_sync.py` writes this file, the skill folders and
`../../src/builtin_skills.rs` from `vstack.json`; edit that list, not these files.

The method and several playbooks are adapted from the pstack plugin
(https://github.com/cursor/plugins/tree/main/pstack). Branding and naming in
the upstream repo are Viper Stack / vstack.

License: MIT. The upstream copyright and permission notice ships unchanged in
[LICENSE-upstream](LICENSE-upstream) in this folder and must stay with these files.

Skills shipped here (read-only built-ins inside Loadout / GrokHub):

- architect
- av-qa-walkthrough
- benchmark-checklist
- blast-radius
- correct
- create-formal-verification-skill
- create-verification-skill
- elevate
- figure-it-out
- follow-through
- how
- in-your-own-words
- interrogate
- ketchup
- land-it
- maintain-formal-verification-skill
- maintain-verification-skill
- optimize
- principle-attack-the-premise
- principle-boundary-discipline
- principle-build-the-lever
- principle-experience-first
- principle-explain-the-number
- principle-fix-root-causes
- principle-foundational-thinking
- principle-make-operations-idempotent
- principle-minimize-reader-load
- principle-model-the-domain
- principle-never-block-on-the-human
- principle-outcome-oriented-execution
- principle-prove-it-works
- principle-redesign-from-first-principles
- principle-separate-before-serializing-shared-state
- principle-sequence-verifiable-units
- principle-subtract-before-you-add
- principle-test-behavior-not-implementation
- principle-type-system-discipline
- qa-walkthrough
- sharpen
- show-me-your-work
- statechart
- swarm
- tdd
- teach
- technical-writing
- typescript-best-practices
- unslop
- viper-mode
- why

Left out:

- bag-and-tag: vstack's own release ritual: it bumps plugin manifests and checks installers other repos don't have
- ketchup-video: renders a slide video and embeds it in chat; the app has no video pipeline for it
- recall: rebuilds context from Cursor's agent-transcripts folder, which the app doesn't keep
- reflect: writes skill edits into Cursor's skill folders, not the app's skill store
- setup-vstack: installs vstack into a coding agent; the app ships these skills itself
- stageshow: ships a 2 MB React kit of example screens beside its text
- updater: runs its 52 KB scripts/updater.py from the skill folder, which is over the skill tool's file size and has no folder on disk to run from

Do not install these as a Cursor plugin from this tree; they are app built-ins.

Bundle SHA-256: `f6e54de8266b7a7ae343d9d65d6d256a50d78f9a09c0d73b86866493a3eace9c`
