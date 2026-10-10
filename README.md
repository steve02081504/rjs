# rjs

`rjs` is a JavaScript, TypeScript and WebAssembly runtime. It is a fork of [Deno](https://github.com/denoland/deno) that I maintain for my own projects, and it is deliberately not a general-purpose platform. There are no permission prompts, no standards committee, and no promises about this repository being a drop-in replacement for anything.

It exists to be fast, to be small in a compiled binary, and to be convenient for the code that actually runs on it.

## What rjs is

rjs is a continuation of an existing runtime, not a new one. The same binary, the same V8, the same npm and Node compatibility layer, the same everything — with the parts I do not want removed or changed wherever that buys performance, smaller output, or less work for the code running on it.

This README describes what rjs is, not what Deno is. Deno's own documentation still documents most of the behavior you will meet here, so use it as a starting point unless something in this file says otherwise.

## The rjs philosophy

These are not aspirations. They are the terms this repository operates on, and they are the reason some things here behave differently from the runtime this fork came from.

### Usable and fast first, standards later

rjs does not treat TC39 proposals, WHATWG specifications or the Web Platform Tests as the authority on what this runtime does. I use them as reference material, and when it is worth it — when the standards-compliant behavior would cost performance, make the code clumsier to write, or make the compiled executable bigger — rjs does something else.

So expect the language and the runtime here to differ from the standard you read about. That is a decision, not a bug, and the only standards I care about are the ones the code people actually run depends on. If a deviation bites you, open an issue; if "because the spec says so" is the whole argument, that is not enough on its own.

### No security theater

rjs runs with everything allowed, by default, always. No `--allow-*` flags, no permission prompts in the middle of a program, no capability model to configure. Code you run here can read, write, spawn, connect and load whatever it can reach, the same way a script in a normal shell can.

This is on purpose and it is not going to change. If you need a sandbox, use one — that is what containers, VMs and operating-system users are for.

### No DEI, no politics

rjs is a technical project. The only criterion for anything here — a patch, a dependency, a contributor, a review — is whether it improves the runtime. Name, pronouns, nationality, employer, gender, who wrote it: all irrelevant. A better patch from an AI beats a worse patch from a person, and I do not care which one I get.

That is also why this repository uses `master`, not `main`. Nothing about the branch name makes the runtime better, so I am not renaming it to make anyone comfortable. Hold on to the things that work; drop the rest.

## Why rjs exists

**Deno has been abandoned as a runtime, and rjs is what replaces it for me.**

The Deno company announced that its team is joining Cloudflare ([deno.com/blog/cloudflare](https://deno.com/blog/cloudflare)). The announcement says the Deno runtime will get another year of monthly bug fixes and security updates, and that development of the runtime ends after that. The repository stays open source and its maintainers welcome someone else continuing it, but as of today nobody has, and the project I depend on is standing at the end of its rope. That discussion is here: [denoland/deno#36995](https://github.com/denoland/deno/issues/36995#issuecomment-6092427360).

Porting to another runtime meant rewriting most of what I actually use, and the runtime I tried could not run my code at all. So I forked Deno instead and kept going with what already worked.

### What is inherited from Deno

The tree is Deno's. V8, Tokio, the extension system, the CLI, the npm registry client, the Node compatibility layer, the test runner, the bundler — all of it, years of work and a lot of genuinely ugly code that I did not have to write, debug or discover. I think of it the way it deserves to be thought of: inherited legacy code that works. Rewriting any of that from scratch would be a multi-year project that ends up worse in every way that matters, written by one person, after the people who knew it best moved on.

So rjs takes the existing runtime and does the work I need on top of it. Expect the runtime underneath to keep being recognizable as Deno for a long time, and expect everything above it to be cut, bent or replaced as I get to it.

### How rjs relates to Deno

rjs is not the successor project. If someone with more hands steps up, or if the runtime finds a new home, I would rather use that and I would hand over anything here that turns out to be useful. The same goes for a second fork: if something else is a better fit for you, use it. rjs is a working copy that keeps my code running and keeps moving in the direction I need.

## What rjs is for

[rjs](https://github.com/steve02081504/rjs) is the runtime for [fount](https://github.com/steve02081504/fount), my project about artificial selves. fount already runs on Deno and uses a large part of the runtime's surface, which is why this fork inherits so much instead of starting over.

What I build here is what fount needs: native clipboard and WebView APIs, a virtual file system, a `git:`-style import prefix, programmatic control over hot reload and the module and package caches, Termux support, a built-in synchronous await, an async `eval`, and so on — plus whatever turns out to be in the way. It is not an attempt to be everything to everybody. It is an opinionated runtime shaped by the code I run on it, and if that happens to be useful to you, good.

## Building and releases

rjs has no version numbers and no release channels. The commit sha *is* the version, and `rjs --version` prints the sha it was built from.

Build from source with the pinned toolchain (`rust-toolchain.toml`):

```sh
cargo build --release --bin rjs
```

[`.github/workflows/build.yml`](.github/workflows/build.yml) runs on every push to `master` and builds `rjs` (plus the `rjsrt` runtime that `rjs compile` needs) for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin` and `x86_64-pc-windows-msvc`. The results are published as assets of the rolling [`default` release](https://github.com/steve02081504/rjs/releases/tag/default); every run overwrites the assets in place, so that release always holds the newest build and nothing else.

`rjs upgrade` reads `rjs-<target>.txt` from that release, compares the sha in it with the sha of the binary it is running, and replaces itself with `rjs-<target>.zip` when they differ. That is the whole update story — there is no stable/canary split and no version to pin.

The tree carries only what building the runtime needs: the test suite, the developer tooling that used to live in `tools/`, the docs and the upstream CI are not here.

## Contributing

Issues and pull requests are welcome. rjs is a personal fork, so I merge what makes the runtime better for the code that runs on it and I am not obliged to accept anything else — see the philosophy above, which is the whole review policy.

Because this fork will diverge from Deno over time, Deno's documentation, test suite and contribution guide describe Deno, not rjs. This tree does not carry a contribution guide of its own — the philosophy above is the whole review policy.

## Credits

rjs is a fork of [denoland/deno](https://github.com/denoland/deno), which stays the upstream project and the source of nearly everything in this tree. Thanks to the Deno team and everyone who contributed there; this fork would not exist without their work. See [LICENSE.md](LICENSE.md) for the license.
