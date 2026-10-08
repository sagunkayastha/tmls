import sys, time
w = sys.stdout.write
w("\x1b[?2004h")                       # bracketed paste on
w("plain header\r\n")
for i in range(0, 101, 5):
    w("\rprogress [%-40s] %3d%%" % ("#" * (i * 40 // 100), i)); sys.stdout.flush(); time.sleep(0.02)
w("\r\n")
w("CJK: 中文宽字符测试 日本語のテキスト 한국어 안녕하세요\r\n")
for _ in range(8):
    w("ＡＢＣ こんにちは世界 " * 4 + "\r\n")
w("emoji ZWJ: \U0001F468‍\U0001F469‍\U0001F467‍\U0001F466 \U0001F469‍\U0001F4BB \U0001F3F3️‍\U0001F308 \U0001F44D\U0001F3FD \U0001F1EF\U0001F1F5\r\n")
w("combining: é ä ñ ộ नमस्ते z̖̗͡ͅ\r\n")
for i in range(0, 256, 1):
    w("\x1b[48;5;%dm  " % i)
    if i % 32 == 31: w("\x1b[0m\r\n")
w("\x1b[0m")
for i in range(0, 120):
    w("\x1b[38;2;%d;%d;%dm█" % (255 - i * 2, i * 2, 128))
w("\x1b[0m\r\n")
w("\x1b[1mbold\x1b[0m \x1b[2mdim\x1b[0m \x1b[3mital\x1b[0m \x1b[4munder\x1b[0m \x1b[7mrev\x1b[0m \x1b[9mstrike\x1b[0m \x1b[4:3m\x1b[58;2;255;0;0mcurly\x1b[0m\r\n")
w("\x1b]8;;https://example.com/a\x1b\\link one\x1b]8;;\x1b\\ and \x1b]8;id=x;https://example.com/b\x07link two\x1b]8;;\x07\r\n")
w("\x1b]0;window title\x07")
w("\x1b[?1049h\x1b[2J\x1b[H")          # alt screen
for r in range(1, 41):
    w("\x1b[%d;1H\x1b[3%dmalt row %02d %s" % (r, r % 8, r, "x" * (r * 2)))
w("\x1b[20;60H\x1b[1;44;33m centre \x1b[0m")
sys.stdout.flush(); time.sleep(0.3)
w("\x1b[?1049l")                       # leave alt screen
w("\x1b[?2004l")                       # bracketed paste off
w("after alt screen\r\n")
for i in range(60):
    w("\x1b[%dmline %d ─│┌┐└┘ \x1b[0m\r\n" % (31 + i % 7, i))
