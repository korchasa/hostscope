# The steps of a check, in the order they cost.
#
# There was no single entry point before, so the set of checks was assembled by
# hand every time. Measured over the session transcripts of this project: 142
# runs of `cargo test`, 130 trips to the host, and a median verification round
# of 5.7 minutes of which the commands were running only 40 percent of the
# time. The rest went on assembling the next command. Three steps, named:
#
#   make fast   after every edit          seconds
#   make live   once per batch of work    minutes, needs the host
#   make all    both
#
# HOST is the machine the live check runs on. It is not the machine the
# application is built on: the build is a musl cross build on the Mac and the
# host needs nothing installed. There is no default: give it on the command
# line, `make live HOST=my.host`, or put `HOST = my.host` into `local.mk`,
# which is read here and is not part of the repository.

-include local.mk

HOST   ?=
DIR    ?= /tmp/hostscope
TARGET := x86_64-unknown-linux-musl
BIN    := target/$(TARGET)/release/hostscope
LOGDIR := target/live

.PHONY: help fast fmt lint test build live live-quick live-bg live-log ship clean

help:
	@echo "make fast        fmt, clippy and the tests           (~15 s)"
	@echo "make live        build, ship and check on $(HOST)  (~4 min)"
	@echo "make live-quick  the sections that raise no load     (~1 min)"
	@echo "make live-bg     the same as live, detached          returns at once"
	@echo "make live-log    the summary of the last live run"
	@echo "make all         fast then live"
	@echo
	@echo "HOST=$(HOST)  DIR=$(DIR)   (set HOST= on the command line or in local.mk)"
	@echo "one section:  make live SECTIONS='oracle security'"

# ---- the fast step -----------------------------------------------------------
# Everything that needs no host and no network. This is the step that runs after
# every edit, so it may not grow past seconds.

fast: fmt lint test

fmt:
	cargo fmt --check

lint:
	cargo clippy --all-targets -- -D warnings

test:
	cargo test

# ---- the live step -----------------------------------------------------------
# One command for what used to be three: build, ship, run, fetch the log. Each
# of those was a separate round trip with a wait and a read in between, and the
# waiting between them cost more than the commands.

build:
	cargo build --release --target $(TARGET)

ship: build
	@HS_HOST=$(HOST) HS_DIR=$(DIR) scripts/live-check.sh --ship-only

live:
	@HS_HOST=$(HOST) HS_DIR=$(DIR) scripts/live-check.sh $(SECTIONS)

# The sections that raise no load on the host: they read, walk the interface and
# lint frames. Safe to run on a busy machine and enough to catch a layout or an
# arithmetic regression.
live-quick:
	@HS_HOST=$(HOST) HS_DIR=$(DIR) scripts/live-check.sh \
		prepare baseline oracle scenario keys linter sizes cleanup

live-bg:
	@HS_HOST=$(HOST) HS_DIR=$(DIR) scripts/live-check.sh --bg $(SECTIONS)

live-log:
	@HS_HOST=$(HOST) HS_DIR=$(DIR) scripts/live-check.sh --log

all: fast live

clean:
	cargo clean
	rm -rf $(LOGDIR)
