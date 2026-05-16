SHELL := /bin/bash
PREFIX ?= $(HOME)/.local
SHARE_DIR := $(PREFIX)/share/codex-session
BIN_DIR := $(PREFIX)/bin
LINK := $(BIN_DIR)/codex-session

PAYLOAD_DIRS := bin lib commands
PAYLOAD_FILES := VERSION

# Portable lexical path normalization (no filesystem touch). Collapses
# leading-anchored `.` and `..` segments so that paths like
# `/a/b/../share/x` become `/a/share/x`. Used by install/uninstall to honor
# the reviewed contract: "managed iff the symlink target resolves into
# $(SHARE_DIR)". Implemented in POSIX awk so it runs on stock macOS/BSD.
define LEXICAL_NORMALIZE
awk -v p="$$1" 'BEGIN { \
  n = split(p, parts, "/"); out = ""; depth = 0; \
  abs = (substr(p, 1, 1) == "/"); \
  for (i = 1; i <= n; i++) { \
    s = parts[i]; \
    if (s == "" || s == ".") continue; \
    if (s == "..") { if (depth > 0) { sub("/[^/]+$$", "", out); depth--; } else if (!abs) { out = (out == "" ? ".." : out "/.."); } continue; } \
    out = (out == "" ? s : out "/" s); depth++; \
  } \
  print (abs ? "/" out : (out == "" ? "." : out)); \
}'
endef

.PHONY: lint test smoke install relink uninstall

lint:
	shellcheck -x bin/codex-session $(wildcard lib/*.bash) $(wildcard commands/*.bash) tests/smoke.sh

test:
	@command -v bats >/dev/null 2>&1 || { echo "bats not found; run: make smoke"; exit 1; }
	bats tests/codex-session.bats

smoke:
	bash tests/smoke.sh

install:
	@mkdir -p "$(SHARE_DIR)" "$(BIN_DIR)"
	@for d in $(PAYLOAD_DIRS); do \
	  rm -rf "$(SHARE_DIR)/$$d"; \
	  cp -R "$$d" "$(SHARE_DIR)/$$d"; \
	done
	@for f in $(PAYLOAD_FILES); do cp "$$f" "$(SHARE_DIR)/$$f"; done
	@chmod +x "$(SHARE_DIR)/bin/codex-session"
	@if [ -e "$(LINK)" ] || [ -L "$(LINK)" ]; then \
	  if [ -L "$(LINK)" ]; then \
	    target="$$(readlink "$(LINK)")"; \
	    case "$$target" in \
	      /*) joined="$$target" ;; \
	      *)  joined="$$(dirname "$(LINK)")/$$target" ;; \
	    esac; \
	    set -- "$$joined"; resolved="$$($(LEXICAL_NORMALIZE))"; \
	    case "$$resolved" in \
	      "$(SHARE_DIR)"/*) echo "managed symlink already in place ($$resolved)"; exit 0 ;; \
	    esac; \
	  fi; \
	  echo "REFUSING: $(LINK) exists and is unmanaged. Run 'make relink' to back it up and replace." >&2; \
	  exit 1; \
	else \
	  ln -s "$(SHARE_DIR)/bin/codex-session" "$(LINK)"; \
	  echo "linked $(LINK) -> $(SHARE_DIR)/bin/codex-session"; \
	fi

relink:
	@if [ -e "$(LINK)" ] || [ -L "$(LINK)" ]; then \
	  ts="$$(date +%Y%m%d-%H%M%S)"; \
	  mv "$(LINK)" "$(LINK).bak.$$ts"; \
	  echo "backed up $(LINK) -> $(LINK).bak.$$ts"; \
	fi
	@ln -s "$(SHARE_DIR)/bin/codex-session" "$(LINK)"
	@echo "linked $(LINK) -> $(SHARE_DIR)/bin/codex-session"

uninstall:
	@if [ -L "$(LINK)" ]; then \
	  target="$$(readlink "$(LINK)")"; \
	  case "$$target" in \
	    /*) joined="$$target" ;; \
	    *)  joined="$$(dirname "$(LINK)")/$$target" ;; \
	  esac; \
	  set -- "$$joined"; resolved="$$($(LEXICAL_NORMALIZE))"; \
	  case "$$resolved" in \
	    "$(SHARE_DIR)"/*) rm "$(LINK)"; echo "removed $(LINK)"; ;; \
	    *) echo "skipping unmanaged $(LINK) (-> $$resolved)" ;; \
	  esac; \
	fi
	@rm -rf "$(SHARE_DIR)"
	@echo "removed $(SHARE_DIR)"
