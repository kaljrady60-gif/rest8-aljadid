#!/usr/bin/env python3
"""Create a traceability index; matches are candidates, not reviewed inventories."""
import argparse, csv, json, re
from collections import defaultdict
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('reference', type=Path, help='extracted, untrusted reference directory')
p.add_argument('output', type=Path)
a = p.parse_args()
registry = json.loads((a.reference / 'assets/data/screens.json').read_text(encoding='utf-8'))
ids = {s['perm_id'] for s in registry['screens']}
hits = defaultdict(list)
for file in sorted((a.reference / 'crates/sa-web/src').rglob('*.rs')):
    for n, line in enumerate(file.read_text(encoding='utf-8').splitlines(), 1):
        for key in re.findall(r'"([a-z][\w.]+)"\s*=>', line):
            if key in ids:
                hits[key].append(f'{file.relative_to(a.reference)}:{n}')
matched = len(hits)
a.output.parent.mkdir(parents=True, exist_ok=True)
with a.output.open('w', encoding='utf-8-sig', newline='') as f:
    w = csv.writer(f)
    w.writerow(['معرّف الشاشة', 'الإدارة في المرجع', 'اسم الشاشة', 'مواقع مطابقة مرشحة للفحص (ليست إثباتا للوظيفة)', 'حالة جرد المكونات'])
    for s in registry['screens']:
        state = 'جرد يدوي أولي - يحتاج استكمال' if s['perm_id'] in {'fin.journal', 'fin.coa'} else 'لم تُجرد المكونات بعد'
        w.writerow([s['perm_id'], s['dept_id'], s['name'], ' | '.join(hits[s['perm_id']]) or 'لم يعثر البحث الآلي على مطابقة مباشرة', state])
print(f"{len(ids)} screens indexed; {matched} with candidate matches; {len(ids)-matched} without direct match; not a component inventory")
