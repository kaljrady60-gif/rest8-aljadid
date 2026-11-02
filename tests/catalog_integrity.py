"""Consistency check of the imported reference registry, not a test of accounting functionality."""
from collections import Counter
from pathlib import Path
import json

j = json.loads((Path(__file__).resolve().parents[1] / 'assets/rest8-catalog.json').read_text(encoding='utf-8'))
screens, departments, sections = j['screens'], j['departments'], j['sections']
ids = [s['perm_id'] for s in screens]
assert len(ids) == len(set(ids)) == 182
assert len(departments) == 8 and len(sections) == 40
assert all(s['dept_id'] in {d['id'] for d in departments} for s in screens)
assert all(ref in ids for section in sections for ref in section['screens'])
assert Counter(s['dept_id'] for s in screens) == {'finance': 41, 'sales': 22, 'purchases': 14, 'inventory': 44, 'reports': 4, 'assets': 6, 'hr': 10, 'system': 41}
print('OK: 182 unique screens, 8 reference departments, 40 sections; all references resolve.')
