# sample.mk — integration corpus fixture for the make extractor
# A realistic project Makefile with variables, pattern rules, and phony targets.

# ---------------------------------------------------------------------------
# Variables
# ---------------------------------------------------------------------------
CC      ?= gcc
CFLAGS  ?= -Wall -Wextra -O2
LDFLAGS ?=
SRCDIR  := src
OBJDIR  := build/obj
BINDIR  := build/bin

SRCS    := $(wildcard $(SRCDIR)/*.c)
OBJS    := $(patsubst $(SRCDIR)/%.c,$(OBJDIR)/%.o,$(SRCS))
TARGET  := $(BINDIR)/myapp

# ---------------------------------------------------------------------------
# Default target
# ---------------------------------------------------------------------------
.PHONY: all
all: $(TARGET)

# ---------------------------------------------------------------------------
# Link
# ---------------------------------------------------------------------------
$(TARGET): $(OBJS) | $(BINDIR)
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $^
	@echo "Linked $@"

# ---------------------------------------------------------------------------
# Pattern rule — compile every .c to .o
# ---------------------------------------------------------------------------
$(OBJDIR)/%.o: $(SRCDIR)/%.c | $(OBJDIR)
	$(CC) $(CFLAGS) -c -o $@ $<

# ---------------------------------------------------------------------------
# Directory scaffolding
# ---------------------------------------------------------------------------
$(OBJDIR) $(BINDIR):
	mkdir -p $@

# ---------------------------------------------------------------------------
# test — build and run the test suite
# ---------------------------------------------------------------------------
.PHONY: test
test: $(TARGET)
	@echo "Running tests…"
	./scripts/run_tests.sh

# ---------------------------------------------------------------------------
# clean — remove all generated artefacts
# ---------------------------------------------------------------------------
.PHONY: clean
clean:
	rm -rf build/

# ---------------------------------------------------------------------------
# install — copy binary to prefix
# ---------------------------------------------------------------------------
PREFIX  ?= /usr/local
.PHONY: install
install: $(TARGET)
	install -d $(DESTDIR)$(PREFIX)/bin
	install -m 755 $(TARGET) $(DESTDIR)$(PREFIX)/bin/

.PHONY: help
help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
	    awk 'BEGIN {FS = ":.*?## "}; {printf "  %-15s %s\n", $$1, $$2}'
