# Static guest helper. Use the Buildroot compiler: the host libc is built for
# x86-64-v4 and the guest kernel rejects those instructions.
CC ?= ../../.build/oseink/host/bin/x86_64-buildroot-linux-gnu-gcc
OUT := ../overlay/bin/oseink-input

$(OUT): oseink-input.c
	$(CC) -Os -s -static -o $(OUT) oseink-input.c

.PHONY: clean
clean:
	rm -f $(OUT)
