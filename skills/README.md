# Zed agent skill

`zed-code-tour/SKILL.md` teaches a Zed agent to write and start code tours with
this extension.

## Install

Zed loads skills from `.agents/skills/<name>/SKILL.md`. Copy the directory:

```sh
# global
mkdir -p ~/.agents/skills
cp -r skills/zed-code-tour ~/.agents/skills/

# or project-local
mkdir -p <project>/.agents/skills
cp -r skills/zed-code-tour <project>/.agents/skills/
```

Start a new agent thread after copying.

## Use

Open the Zed agent panel and type `/zed-code-tour`, or pick it from the slash
menu. The model can also invoke it when the request matches the description.

## Notes

- The extension and `tour-lsp` still have to be installed separately. See
  [docs/install.md](../docs/install.md).
- If the skill does not appear, check that the file is exactly
  `<root>/zed-code-tour/SKILL.md` with YAML frontmatter containing `name` and
  `description`.
