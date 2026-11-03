#!/usr/bin/env python3
"""Tiny converter for docs/portable/quick-start-guide.md -> quick-start-guide.html (headings, lists, code, tables, bold, inline code, emphasis)."""
import html, re, sys

def inline(t):
    t = html.escape(t)
    t = re.sub(r'`([^`]+)`', r'<code>\1</code>', t)
    t = re.sub(r'\*\*([^*]+)\*\*', r'<strong>\1</strong>', t)
    t = re.sub(r'\*([^*]+)\*', r'<em>\1</em>', t)
    return t

def convert(md):
    out, i, lines = [], 0, md.split('\n')
    title = 'NanoNexus quick start'
    while i < len(lines):
        l = lines[i]
        if l.startswith('```'):
            j = i + 1; buf = []
            while j < len(lines) and not lines[j].startswith('```'):
                buf.append(lines[j]); j += 1
            out.append('<pre><code>' + html.escape('\n'.join(buf)) + '</code></pre>'); i = j + 1; continue
        m = re.match(r'(#{1,3}) (.*)', l)
        if m:
            n = len(m.group(1))
            if n == 1: title = m.group(2)
            out.append(f'<h{n}>{inline(m.group(2))}</h{n}>'); i += 1; continue
        if l.startswith('|'):
            rows = []
            while i < len(lines) and lines[i].startswith('|'):
                rows.append([c.strip() for c in lines[i].strip('|').split('|')]); i += 1
            rows = [r for r in rows if not all(re.fullmatch(r'-+', c) for c in r)]
            out.append('<table><thead><tr>' + ''.join(f'<th>{inline(c)}</th>' for c in rows[0]) + '</tr></thead><tbody>' +
                       ''.join('<tr>' + ''.join(f'<td>{inline(c)}</td>' for c in r) + '</tr>' for r in rows[1:]) + '</tbody></table>'); continue
        if re.match(r'(- |\d+\. )', l):
            tag = 'ol' if l[0].isdigit() else 'ul'
            items = []
            while i < len(lines) and re.match(r'(- |\d+\. )', lines[i]):
                items.append(re.sub(r'^(- |\d+\. )', '', lines[i])); i += 1
            out.append(f'<{tag}>' + ''.join(f'<li>{inline(x)}</li>' for x in items) + f'</{tag}>'); continue
        if l.strip():
            out.append(f'<p>{inline(l)}</p>')
        i += 1
    return title, '\n'.join(out)

src, dst = sys.argv[1], sys.argv[2]
title, body = convert(open(src, encoding='utf-8').read())
page = f'''<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(title)}</title>
<style>
:root{{--bg:#fff;--fg:#1c1e21;--muted:#5b6370;--line:#d9dde3;--code:#f3f4f6;--accent:#2457d6}}
@media (prefers-color-scheme:dark){{:root{{--bg:#15171a;--fg:#e8eaed;--muted:#a0a6b0;--line:#2c3036;--code:#1f2227;--accent:#7aa2ff}}}}
body{{margin:0;background:var(--bg);color:var(--fg);font:16px/1.6 system-ui,sans-serif}}
main{{max-width:760px;margin:0 auto;padding:24px 16px 64px}}
h1{{font-size:1.8rem}}h2{{margin-top:2rem;border-top:1px solid var(--line);padding-top:1rem}}h3{{margin-top:1.2rem}}
pre{{background:var(--code);padding:12px;border-radius:8px;overflow-x:auto}}code{{background:var(--code);padding:1px 5px;border-radius:4px;font-size:.92em}}pre code{{padding:0;background:none}}
table{{border-collapse:collapse;width:100%}}th,td{{border:1px solid var(--line);padding:6px 10px;text-align:left}}
a{{color:var(--accent)}}
</style></head><body><main>
{body}
</main></body></html>
'''
open(dst, 'w', encoding='utf-8').write(page)
