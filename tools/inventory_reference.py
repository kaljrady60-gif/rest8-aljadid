#!/usr/bin/env python3
"""Generate a static navigation inventory; references are leads, not proof of behavior."""
from pathlib import Path
import csv
import html
import json
import re

ROOT = Path(__file__).resolve().parents[1]
REFERENCE = Path('/home/user/rest8-reference')
WEB = REFERENCE / 'crates/sa-web'
OUTPUT = ROOT / 'docs/reference-inventory'
CATALOG = ROOT / 'assets/rest8-catalog.json'


def main():
    data = json.loads(CATALOG.read_text(encoding='utf-8'))
    screens = data['screens']
    pattern = re.compile(r'(?<![\w.])(?:' + '|'.join(
        re.escape(s['perm_id']) for s in sorted(screens, key=lambda x: -len(x['perm_id']))
    ) + r')(?![\w.])')
    matches = {s['perm_id']: [] for s in screens}
    tabs = []
    files = sorted(p for root in (WEB / 'src', WEB / 'templates')
                   for p in root.rglob('*') if p.is_file() and p.suffix in {'.rs', '.html'})
    for file in files:
        path = file.relative_to(REFERENCE).as_posix()
        text = file.read_text(encoding='utf-8')
        for line, content in enumerate(text.splitlines(), 1):
            for key in set(pattern.findall(content)):
                if len(matches[key]) < 12:
                    matches[key].append(f'{path}:{line}')
            for match in re.finditer(r'data-subtab\s*=\s*"([^"]+)"', content):
                value = match.group(1)
                tail = content[match.end():]
                name = ''
                if file.suffix == '.html':
                    name_match = re.search(r'>\s*([^<]+)\s*<', tail)
                else:
                    name_match = re.search(r'\{\s*"([^"]+)"', tail)
                if name_match:
                    name = html.unescape(name_match.group(1).strip())
                tabs.append((path, line, value, name, 'مطابقة آلية؛ يحتاج ربطًا يدويًا بالسياق'))
        # Rust/Maud screens also define arrays passed to the common u::subtabs helper.
        for group in re.finditer(r'u::subtabs\(\s*&\s*\[(.*?)\]', text, re.S):
            first_line = text.count('\n', 0, group.start(1)) + 1
            for match in re.finditer(r'\(\s*"([^"\n]+)"\s*,\s*"([^"\n]+)"\s*\)', group.group(1)):
                line = first_line + group.group(1).count('\n', 0, match.start())
                tabs.append((path, line, match.group(1), match.group(2),
                             'مصفوفة ثابتة؛ يحتاج ربطًا يدويًا بالسياق'))
    OUTPUT.mkdir(parents=True, exist_ok=True)
    with (OUTPUT / 'الشاشات_والأقسام.csv').open('w', encoding='utf-8-sig', newline='') as f:
        w = csv.writer(f)
        w.writerow(['معرف', 'الإدارة', 'الشاشة', 'الأقسام', 'مواضع محتملة في الخادم والقوالب', 'حالة الفحص'])
        for screen in screens:
            sections = ' | '.join(s['name'] for s in data['sections'] if screen['perm_id'] in s['screens'])
            w.writerow([screen['perm_id'], screen['dept_id'], screen['name'], sections,
                        ' | '.join(matches[screen['perm_id']]), 'فهرسة آلية فقط؛ الوظيفة والتبويبات تحتاج فحصًا'])
    with (OUTPUT / 'مرشحات_التبويبات_الفرعية.csv').open('w', encoding='utf-8-sig', newline='') as f:
        w = csv.writer(f)
        w.writerow(['الملف المرجعي', 'السطر', 'معرف التبويب', 'النص إن أمكن', 'التحقق'])
        w.writerows(tabs)
    print(f'{len(screens)} screens; {len(data["sections"])} sections; {len(files)} reference files; {len(tabs)} static subtab candidates')
    print('screens with direct source references:', sum(bool(v) for v in matches.values()))


if __name__ == '__main__':
    main()
