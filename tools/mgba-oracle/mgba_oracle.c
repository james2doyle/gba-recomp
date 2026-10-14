/*
 * mgba-oracle: external reference runner for the frame differential against
 * gba-core (SPEC-011 VAL-03, ADR-041 item 4).
 *
 * This is a test tool, not product code. It only calls libmgba's public API
 * (mGBA is MPL-2.0; no mGBA source is copied here or into product crates).
 *
 * CLI mirrors gba-run:
 *   mgba-oracle --rom R [--frames N] [--input FRAME:LEN:KEYS ...]
 *               [--ppm prefix.ppm] [--ppm-every N] [--ppm-window K] [--hashes]
 *
 * Semantics kept identical to gba-run:
 *   - keys for frame f are the OR of every --input with START <= f < START+LEN,
 *     set before running frame f (0-based);
 *   - after frame f completes the 1-based counter is f+1 and, when
 *     (f+1) % every == 0, "<prefix>.<f+1>.ppm" is written (Path::with_extension
 *     semantics: the last extension of --ppm is replaced);
 *   - --ppm-window K (oracle-only extension) additionally writes every frame
 *     within +-K of each multiple of --ppm-every, so scripts/diff-frames.py can
 *     search for the best-matching reference frame when timing drifts;
 *   - --dump-state FRAME (repeatable, oracle-only) writes "<--dump-dir>/state.<FRAME>.bin"
 *     after that 1-based frame: IO 0x400 | palette 0x400 | VRAM 0x18000 | OAM 0x400
 *     bytes, little-endian, as stored by mGBA (debug aid; ROM-derived, local only);
 *   - --ppm alone writes the final frame to that path;
 *   - --hashes prints "frame N hash H" to stderr, H = FNV-1a 64 over the
 *     framebuffer as little-endian BGR555 u16 (gba-core AV-01 hash).
 *
 * Color: libmgba must be built with COLOR_16_BIT and without COLOR_5_6_5
 * (scripts/build-mgba-oracle.sh). mColor is then the raw GBA BGR555 value
 * (bits 0-4 red, 5-9 green, 10-14 blue) and blending/brightness are computed in
 * 5-bit precision, as on hardware. In the default 32-bit build mGBA blends in
 * 8-bit precision, which would make blended pixels differ in low bits. Each
 * channel is expanded to 8 bits exactly like gba-run: (v << 3) | (v >> 2).
 * Bit 15 is masked off.
 *
 * BIOS: no BIOS file is loaded (useBios=0, skipBios=1), so mGBA uses its
 * built-in HLE BIOS and starts at the ROM entry point after GBASkipBIOS, like
 * gba-core's HLE BIOS post-boot state. Idle-loop optimization is disabled.
 * The save is in-memory only (nothing is written to disk).
 */
#include <mgba/flags.h>
#include <mgba/core/config.h>
#include <mgba/core/core.h>
#include <mgba/core/log.h>
#include <mgba/core/version.h>
#include <mgba/internal/gba/gba.h>
#include <mgba-util/image.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <time.h>

#if !defined(COLOR_16_BIT) || defined(COLOR_5_6_5)
#error "libmgba must be built with COLOR_16_BIT and without COLOR_5_6_5"
#endif

#define WIDTH 240
#define HEIGHT 160
#define MAX_INPUTS 256

struct input {
	unsigned long long start, len;
	uint32_t keys;
};

static void usage(void) {
	fprintf(stderr,
	        "usage: mgba-oracle --rom <rom.gba> [--frames N] [--input FRAME:LEN:KEYS ...] "
	        "[--ppm out.ppm] [--ppm-every N] [--ppm-window K] [--hashes]\n"
	        "       [--dump-dir DIR --dump-state FRAME ...]\n"
	        "KEYS: comma-separated A,B,SELECT,START,RIGHT,LEFT,UP,DOWN,R,L\n");
}

static int parse_keys(const char* text, uint32_t* out) {
	static const char* names[] = { "A", "B", "SELECT", "START", "RIGHT", "LEFT", "UP", "DOWN", "R", "L" };
	char buf[256];
	snprintf(buf, sizeof(buf), "%s", text);
	uint32_t mask = 0;
	for (char* tok = strtok(buf, ","); tok; tok = strtok(NULL, ",")) {
		int found = -1;
		for (int i = 0; i < 10; ++i) {
			if (strcasecmp(tok, names[i]) == 0) {
				found = i;
			}
		}
		if (found < 0) {
			fprintf(stderr, "mgba-oracle: unknown key `%s`\n", tok);
			return -1;
		}
		mask |= 1u << found;
	}
	*out = mask;
	return 0;
}

/* Replace the last extension of `path` by `ext` (Rust Path::with_extension). */
static void with_extension(char* out, size_t size, const char* path, const char* ext) {
	const char* slash = strrchr(path, '/');
	const char* base = slash ? slash + 1 : path;
	const char* dot = strrchr(base, '.');
	size_t stem = dot && dot != base ? (size_t) (dot - path) : strlen(path);
	snprintf(out, size, "%.*s.%s", (int) stem, path, ext);
}

static int write_ppm(const char* path, const mColor* fb) {
	FILE* f = fopen(path, "wb");
	if (!f) {
		fprintf(stderr, "mgba-oracle: cannot write %s\n", path);
		return -1;
	}
	fprintf(f, "P6\n%d %d\n255\n", WIDTH, HEIGHT);
	static uint8_t row[WIDTH * 3];
	for (int y = 0; y < HEIGHT; ++y) {
		for (int x = 0; x < WIDTH; ++x) {
			uint16_t c = fb[y * WIDTH + x] & 0x7FFF;
			for (int ch = 0; ch < 3; ++ch) {
				uint8_t v = (c >> (5 * ch)) & 0x1F;
				row[x * 3 + ch] = (uint8_t) ((v << 3) | (v >> 2));
			}
		}
		fwrite(row, 1, sizeof(row), f);
	}
	return fclose(f) == 0 ? 0 : -1;
}

static uint64_t frame_hash(const mColor* fb) {
	uint64_t h = 0xCBF29CE484222325ULL;
	for (int i = 0; i < WIDTH * HEIGHT; ++i) {
		uint16_t c = fb[i] & 0x7FFF;
		h ^= c & 0xFF;
		h *= 0x100000001B3ULL;
		h ^= c >> 8;
		h *= 0x100000001B3ULL;
	}
	return h;
}

static int dump_state(const char* dir, unsigned long long frame, struct mCore* core) {
	struct GBA* gba = core->board;
	char path[4096];
	snprintf(path, sizeof(path), "%s/state.%llu.bin", dir, frame);
	FILE* f = fopen(path, "wb");
	if (!f) {
		fprintf(stderr, "mgba-oracle: cannot write %s\n", path);
		return -1;
	}
	/* Host is little-endian (arm64/x86-64), so the u16 arrays are written as-is. */
	fwrite(gba->memory.io, 1, GBA_SIZE_IO, f);
	fwrite(gba->video.palette, 1, GBA_SIZE_PALETTE_RAM, f);
	fwrite(gba->video.vram, 1, GBA_SIZE_VRAM, f);
	fwrite(gba->video.oam.raw, 1, GBA_SIZE_OAM, f);
	return fclose(f) == 0 ? 0 : -1;
}

static void quiet_log(struct mLogger* logger, int category, enum mLogLevel level, const char* format, va_list args) {
	(void) logger;
	(void) category;
	(void) level;
	(void) format;
	(void) args;
}

int main(int argc, char** argv) {
	const char* rom = NULL;
	const char* ppm = NULL;
	unsigned long long frames = 60, every = 0, window = 0;
	int hashes = 0;
	static struct input inputs[MAX_INPUTS];
	size_t ninputs = 0;
	const char* dump_dir = ".";
	static unsigned long long dumps[MAX_INPUTS];
	size_t ndumps = 0;

	for (int i = 1; i < argc; ++i) {
		const char* flag = argv[i];
		if (strcmp(flag, "--hashes") == 0) {
			hashes = 1;
			continue;
		}
		if (i + 1 >= argc) {
			usage();
			return 1;
		}
		const char* value = argv[++i];
		if (strcmp(flag, "--rom") == 0) {
			rom = value;
		} else if (strcmp(flag, "--frames") == 0) {
			frames = strtoull(value, NULL, 10);
		} else if (strcmp(flag, "--ppm") == 0) {
			ppm = value;
		} else if (strcmp(flag, "--ppm-every") == 0) {
			every = strtoull(value, NULL, 10);
		} else if (strcmp(flag, "--dump-dir") == 0) {
			dump_dir = value;
		} else if (strcmp(flag, "--dump-state") == 0) {
			if (ndumps >= MAX_INPUTS) {
				fprintf(stderr, "mgba-oracle: too many --dump-state\n");
				return 1;
			}
			dumps[ndumps++] = strtoull(value, NULL, 10);
		} else if (strcmp(flag, "--ppm-window") == 0) {
			window = strtoull(value, NULL, 10);
		} else if (strcmp(flag, "--input") == 0) {
			unsigned long long start, len;
			int consumed = 0;
			if (ninputs >= MAX_INPUTS || sscanf(value, "%llu:%llu:%n", &start, &len, &consumed) != 2 || !consumed) {
				fprintf(stderr, "mgba-oracle: bad --input `%s`\n", value);
				return 1;
			}
			inputs[ninputs].start = start;
			inputs[ninputs].len = len;
			if (parse_keys(value + consumed, &inputs[ninputs].keys) < 0) {
				return 1;
			}
			++ninputs;
		} else {
			fprintf(stderr, "mgba-oracle: unknown argument `%s`\n", flag);
			usage();
			return 1;
		}
	}
	if (!rom) {
		usage();
		return 1;
	}

	static struct mLogger logger = { .log = quiet_log };
	mLogSetDefaultLogger(&logger);

	struct mCore* core = mCoreFind(rom);
	if (!core || core->platform(core) != mPLATFORM_GBA) {
		fprintf(stderr, "mgba-oracle: no GBA core for %s\n", rom);
		return 1;
	}
	core->init(core);
	static mColor fb[WIDTH * HEIGHT];
	core->setVideoBuffer(core, fb, WIDTH);
	if (!mCoreLoadFile(core, rom)) {
		fprintf(stderr, "mgba-oracle: cannot load %s\n", rom);
		return 1;
	}
	/* Config is never loaded from or saved to disk. */
	mCoreConfigInit(&core->config, NULL);
	mCoreConfigSetIntValue(&core->config, "useBios", 0);
	mCoreConfigSetIntValue(&core->config, "skipBios", 1);
	mCoreConfigSetValue(&core->config, "idleOptimization", "ignore");
	mCoreConfigSetIntValue(&core->config, "frameskip", 0);
	mCoreLoadConfig(core);
	core->reset(core);

	struct timespec t0, t1;
	clock_gettime(CLOCK_MONOTONIC, &t0);
	unsigned long long frame = 0;
	char path[4096], ext[64];
	while (frame < frames) {
		uint32_t pressed = 0;
		for (size_t k = 0; k < ninputs; ++k) {
			if (frame >= inputs[k].start && frame < inputs[k].start + inputs[k].len) {
				pressed |= inputs[k].keys;
			}
		}
		core->setKeys(core, pressed);
		core->runFrame(core);
		++frame;
		for (size_t k = 0; k < ndumps; ++k) {
			if (dumps[k] == frame && dump_state(dump_dir, frame, core) < 0) {
				return 1;
			}
		}
		if (hashes) {
			fprintf(stderr, "frame %llu hash %016llx\n", frame, (unsigned long long) frame_hash(fb));
		}
		unsigned long long near = 0;
		if (every) {
			unsigned long long r = frame % every;
			near = r <= window || every - r <= window;
		}
		if (every && ppm && near) {
			snprintf(ext, sizeof(ext), "%llu.ppm", frame);
			with_extension(path, sizeof(path), ppm, ext);
			if (write_ppm(path, fb) < 0) {
				return 1;
			}
		}
	}
	clock_gettime(CLOCK_MONOTONIC, &t1);
	double elapsed = (double) (t1.tv_sec - t0.tv_sec) + (double) (t1.tv_nsec - t0.tv_nsec) / 1e9;
	if (ppm && write_ppm(ppm, fb) < 0) {
		return 1;
	}

	int32_t pc = 0, cpsr = 0;
	core->readRegister(core, "pc", &pc);
	core->readRegister(core, "cpsr", &cpsr);
	printf("{\"oracle\":\"mgba\",\"mgba_version\":\"%s\",\"color\":\"bgr555-16bit\",\"bios\":\"hle\","
	       "\"frames\":%llu,\"frame_counter\":%u,\"frame_hash\":\"%016llx\",\"pc\":\"0x%08X\",\"cpsr\":\"0x%08X\","
	       "\"host_seconds\":%.3f,\"fps\":%.1f}\n",
	       projectVersion, frame, core->frameCounter(core), (unsigned long long) frame_hash(fb), (uint32_t) pc,
	       (uint32_t) cpsr, elapsed, elapsed > 0 ? (double) frame / elapsed : 0.0);

	mCoreConfigDeinit(&core->config);
	core->deinit(core);
	return 0;
}
