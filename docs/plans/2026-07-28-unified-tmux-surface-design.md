# Unified tmux Surface Design

## Outcome

`Alt-A` opens AIKit's primary interface as a real tmux `display-popup` occupying
82% of the client width and 70% of its height. The popup owns one continuous
AIKit surface: the invocation palette opens first and `Ctrl-T` toggles the
organising tree without tearing down the terminal, rediscovering the context, or
discarding interaction state.

The interface remains a transient palette rather than a dashboard. Closing it
returns to the underlying pane unchanged.

## Surface model

The TUI gains a surface coordinator above the existing palette and tree models.
It owns:

- one terminal lifecycle;
- one live application service;
- palette state;
- tree state and interaction state;
- the active mode (`Palette` or `Tree`);
- one staged activation set shared by both modes.

The palette and tree remain pure reducer-driven components. Terminal events are
routed to the active reducer. A mode-switch action changes only the coordinator;
the inactive mode remains resident and resumes exactly where the user left it.

`Ctrl-T` toggles palette/tree. `Esc` first dismisses the active modal or prompt,
then returns from tree to palette, and closes the popup only from the palette's
resting search state. Selecting a runnable tree leaf transfers its typed capsule
identity into the palette's existing preview, trust, argument-form, and run
flow.

## State and mutation flow

The palette's staged set becomes the surface's canonical staged state. Tree
toggles update that same set, and palette staging is projected back into tree
marks. Scope is shared as well, so staged changes cannot silently move between
session and durable profiles when the user changes modes.

Applying changes uses the existing application service and Procedure runner.
Success refreshes the resolved generation, search rows, tree model, client
effects, and staged marks while keeping the popup open with a visible result.
Failure keeps the popup open, preserves every staged item, and renders the stable
error code and message in the active mode.

Tree management effects—create, rename, delete, add, and remove—run through the
same Procedure paths already used by the CLI. After each successful effect the
tree is rebuilt without recreating the surface. Foreground or replace execution
returns a typed run intent; the popup restores the terminal and closes before
the command starts. New-pane/new-view execution is handed to the mux adapter
after restoration.

## tmux integration

`aikit mux install tmux` manages a marked block in `~/.tmux.conf`. The block
binds the selected key (default `M-a`) to:

```tmux
display-popup -E -w 82% -h 70% -T AIKit 'aikit ui'
```

Installation is planned, reviewable, reversible, and idempotent. Before apply,
AIKit inspects the effective tmux key table and refuses to steal a conflicting
binding unless the user explicitly chooses a replacement. After writing the
block, AIKit reloads the running tmux server and verifies the live binding
contains the expected popup command. A missing server is not an installation
failure: the configuration is verified on disk and will load with the next
server.

The popup inherits the invoking pane's working directory and tmux environment,
so context resolution remains tied to the work the user was doing.

## Rendering and accessibility

The popup uses the existing wide/medium/narrow layouts within its actual
viewport. It retains the one-border visual system, native terminal palette,
complete ASCII fallback, stable text labels, and no Nerd Font dependency. Tree
keyboard and mouse operations continue through one reducer. The palette remains
keyboard-first as specified.

The header identifies the current context, scope, active mode, and staged count.
The footer shows only keys valid in the current mode, including the `Ctrl-T`
mode switch.

## Verification

Development is test-first. Reducer tests prove state survives mode switches and
that both modes share staging and scope. Render snapshots cover palette and tree
inside the popup at representative sizes. Real-binary PTY tests drive mode
switching, tree staging, palette review, and apply through one terminal
lifecycle.

A real isolated tmux server verifies:

1. the managed binding opens `display-popup` with the specified geometry;
2. the popup inherits the source pane directory;
3. `Ctrl-T` switches modes without terminating the popup process;
4. closing restores the underlying pane;
5. reload makes the installed binding live;
6. conflicting bindings are not overwritten.

The full workspace test suite, strict clippy, and measured cold/warm first-paint
and search-latency checks gate completion.
