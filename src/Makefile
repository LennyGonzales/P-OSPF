TARGET ?= ifshow

CC = gcc
CFLAGS = -static -Wall -Wextra -O2

BIN = $(TARGET)

all: $(BIN)

$(BIN):
	$(CC) $(CFLAGS) -o bin/neighborshow neighborshow.c
	$(CC) $(CFLAGS) -o bin/neighborshow-agent neighborshow-agent.c

clean:
	rm -f $(BIN)
.PHONY: clean

install:
	cp $(BIN) /usr/local/bin/
.PHONY: install

uninstall:
	rm -f /usr/local/bin/$(BIN)
.PHONY: uninstall
