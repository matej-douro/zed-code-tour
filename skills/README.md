# Zed agent skill

`SKILL.md` contains a ready-to-install skill that teaches a Zed agent to write
and start code tours with this extension.

## Why it is not installed automatically

Zed loads skills from:

- `~/.agents/skills/<name>/SKILL.md` (global), and
- `<project>/.agents/skills/<name>/SKILL.md` (project-local).

Extensions cannot contribute skills. The extension manifest has no `skills`
entry and the released `zed_extension_api` has no skill hook, so this repository
cannot register the skill when the extension is installed. Distribution is a
file copy.

Extensions can contribute slash commands, but a slash command is a user-invoked
prompt fragment. It is not part of the model's skill catalog and does not get the
description-based activation or the `skill` tool behavior, so a copyable skill
file is the right shape here.

Zed discovers skills one level deep: the immediate child directory of the
skills root must contain `SKILL.md`. The repository keeps the skill at
`skills/SKILL.md` for download, so copy it into a `zed-code-tour/` directory.

## Install globally

```sh
mkdir -p ~/.agents/skills/zed-code-tour
cp skills/SKILL.md ~/.agents/skills/zed-code-tour/SKILL.md
```

## Install in one project

```sh
mkdir -p <project>/.agents/skills/zed-code-tour
cp skills/SKILL.md <project>/.agents/skills/zed-code-tour/SKILL.md
```

The project-local copy is available only when the project is trusted. Start a
new agent thread after copying so the skill catalog refreshes.

## Use it

Open the Zed agent panel and type `/zed-code-tour`, or pick the skill from the
slash-command menu. The model can also invoke it on its own when the request
matches the skill description.

The directory name can be anything; Zed uses the `name` field in the
frontmatter (`zed-code-tour`). Keep the directory name the same for clarity.

## Notes

- The skill is documentation for the agent. It does not add tools, MCP servers,
  or runtime behavior.
- The extension and `tour-lsp` still have to be installed separately. See
  [docs/install.md](../docs/install.md).
- If the skill does not appear, check that the file is exactly
  `<root>/zed-code-tour/SKILL.md`, that it starts with `---`, and that the
  frontmatter contains `name` and `description`.
