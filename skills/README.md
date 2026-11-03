# Zed agent skill

`zed-code-tour/SKILL.md` contains a ready-to-install skill that teaches a Zed
agent to write and start code tours with this extension.

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

The repository uses Zed's one-directory-per-skill layout, so copy the
`zed-code-tour` directory into a skills root.

## Install globally

```sh
mkdir -p ~/.agents/skills
cp -r skills/zed-code-tour ~/.agents/skills/
```

## Install in one project

```sh
mkdir -p <project>/.agents/skills
cp -r skills/zed-code-tour <project>/.agents/skills/
```

The project-local copy is available only when the project is trusted. Start a
new agent thread after copying so the skill catalog refreshes.

## Use it

Open the Zed agent panel and type `/zed-code-tour`, or pick the skill from the
slash-command menu. The model can also invoke it on its own when the request
matches the skill description.

Zed reads the skill name from the frontmatter. Keep the directory name equal to
the frontmatter name (`zed-code-tour`) so the layout matches the expected skill
format.

## Notes

- The skill is documentation for the agent. It does not add tools, MCP servers,
  or runtime behavior.
- The extension and `tour-lsp` still have to be installed separately. See
  [docs/install.md](../docs/install.md).
- If the skill does not appear, check that the file is exactly
  `<root>/zed-code-tour/SKILL.md`, that it starts with `---`, and that the
  frontmatter contains `name` and `description`.
