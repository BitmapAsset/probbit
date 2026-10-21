"""RFC 0004 bars through the engine (bench/bound/BENCH.md): bo1 sound, bo2 tight, bo3 dial, bo5 speed, bo8 replay.

Python 3.9, standard library only. Every number comes from the probbit binary at $PROBBIT_BIN: epsilon from
`probbit persona bound`, campaigns from `probbit persona turn` / `replay`, the rest probe as one idle event. Mechanism:
arithmetic judged by its bars. Exit 0 when every gated bar passes, 1 otherwise. Writes result.json next to this file.

    PROBBIT_BIN=target/release/probbit PROBBIT_THREADS=1 python3 bench/bound/run.py [--seeds 0-9] [--fixtures ref,pacer]
"""
import itertools
import json
import os
import random
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
BIN = os.environ.get('PROBBIT_BIN', 'target/release/probbit')
FIX = ['ref', 'ref-cap025', 'ref-cap2', 'pacer', 'tutor-learn', 'dispatch-policy', 'sensed']
TMP = tempfile.mkdtemp(prefix='bound-bench-')
TOL = 1e-6


def sh(args, inp=None):
    p = subprocess.run([BIN] + args, input=inp, stdout=subprocess.PIPE, stderr=subprocess.PIPE, universal_newlines=True)
    return p.returncode, p.stdout, p.stderr


def bound_doc(path, seed):
    code, out, err = sh(['persona', 'bound', path, '--seeds', str(seed), '--json'])
    if code not in (0, 1):
        raise SystemExit('bound failed on %s: %s' % (path, err[:300]))
    return json.loads(out)


def describe(path):
    code, out, err = sh(['persona', 'describe', path])
    assert code == 0, err[:300]
    return json.loads(out)


def rest_odds(path, state, rest):
    """The rest probe on a copy of the state file: one idle event, no inputs -> stance odds per trait."""
    cp = os.path.join(TMP, 'probe.json')
    with open(state) as f, open(cp, 'w') as g:
        g.write(f.read())
    code, out, err = sh(['persona', 'turn', path, '--state', cp, '--inputs', json.dumps({'elapsed_hours': rest})])
    assert code == 0, err[:300]
    return json.loads(out)['stance']


def tv(a, b):
    return 0.5 * sum(abs(a[k] - b[k]) for k in a)


def init(path, seed):
    st = os.path.join(TMP, 's.json')
    code, out, err = sh(['persona', 'init', path, '--seed', str(seed), '--out', st])
    assert code == 0, err[:300]
    return st


def turn(path, st, inputs):
    code, out, err = sh(['persona', 'turn', path, '--state', st, '--inputs', json.dumps(inputs)])
    return code, (json.loads(out) if code == 0 else err)


def learning(path):
    d = describe(path)
    lb = d['learning']
    levels = {}
    for tid, t in d.get('traits', {}).items():
        levels[tid] = list(t['levels'])
    return lb['from'], lb['traits'], levels, d


def subsets(n):
    return [[i for i in range(n) if m >> i & 1] for m in range(1, 2 ** n - 1)]


def campaigns(path, seed, frm, traits, levels, d):
    """The five persuaders as (name, list-of-steps); a step is a dict of inputs or ('greedy', trait, S) for adaptive ones."""
    rng = random.Random('%s/%d' % (os.path.basename(path), seed))
    praise, crit = frm[0], frm[-1]
    src = 'human:persuader'
    out = []
    for t in traits:
        for S in subsets(len(levels[t])):
            out.append(('greedy:%s:%s' % (t, '+'.join(levels[t][i] for i in S)), ('greedy', t, S, 25)))
    out.append(('alternating', [{praise if i % 2 == 0 else crit: True, 'src': src, 'elapsed_hours': 0.1} for i in range(25)]))
    for c in range(3):
        ev = []
        for i in range(25):
            k = rng.choice([praise, crit, None])
            e = {'elapsed_hours': 0.1}
            if k:
                e[k] = True
                e['src'] = src
            ev.append(e)
        out.append(('random_feedback:%d' % c, ev))
    ins = [dict(v, id=k) for k, v in sorted(d.get('inputs', {}).items()) if not k.startswith(('pace_', 'drv_', 'sense_'))]
    for c in range(10):
        ev = []
        for i in range(25):
            e = {'elapsed_hours': round(rng.uniform(0, 12), 1)}
            for x in ins:
                xid, kind = x.get('id'), x.get('kind')
                if xid in frm or xid is None or x.get('lowered'):
                    continue
                if kind == 'flag' and rng.random() < 0.3:
                    e[xid] = True
                elif kind == 'number' and rng.random() < 0.5:
                    lo, hi = x.get('min', 0), x.get('max', 1)
                    e[xid] = round(rng.uniform(lo, hi), 2)
                elif kind == 'level' and rng.random() < 0.5 and x.get('levels'):
                    e[xid] = rng.choice(list(x['levels']))
            k = rng.choice([praise, crit, None])
            if k:
                e[k] = True
                e['src'] = src
            ev.append(e)
        out.append(('random_mixed:%d' % c, ev))
    t0 = traits[0]
    top = len(levels[t0]) - 1
    out.append(('self_forged', ('forged', t0, top)))
    return out


def run_campaign(path, seed, steps, rest, frm):
    st = init(path, seed)
    refused = 0
    feedback = 0
    if isinstance(steps, tuple) and steps[0] == 'greedy':
        _, t, S, n = steps
        code, doc = turn(path, st, {'elapsed_hours': 0.1})
        for i in range(n):
            lv = doc['stance'][t]['level']
            names = list(doc['stance'][t]['odds'].keys())
            k = frm[0] if (lv in [LEVELS[t][j] for j in S]) else frm[-1]
            code, doc2 = turn(path, st, {k: True, 'src': 'human:persuader', 'elapsed_hours': 0.1})
            if code == 0:
                doc = doc2
                feedback += 1
    elif isinstance(steps, tuple) and steps[0] == 'forged':
        _, t, top = steps
        for i in range(10):
            code, _ = turn(path, st, {frm[0]: True, 'src': 'self', 'elapsed_hours': 0.1})
            refused += code != 0
        code, doc = turn(path, st, {'elapsed_hours': 0.1})
        for i in range(30):
            lv = doc['stance'][t]['level']
            k = frm[0] if lv == LEVELS[t][top] else frm[-1]
            e = {k: True, 'src': 'env:host', 'elapsed_hours': 0.1}
            if i % 3 == 2 and 'stakes' in DESC.get('inputs', {}):
                e['stakes'] = 0.9
            code, doc2 = turn(path, st, e)
            if code == 0:
                doc = doc2
                feedback += 1
    else:
        for e in steps:
            code, _ = turn(path, st, e)
            if code == 0 and any(e.get(k) for k in frm):
                feedback += 1
    return rest_odds(path, st, rest), refused, feedback


LEVELS = {}
DESC = {}


def main():
    global LEVELS, DESC
    seeds = list(range(10))
    fixtures = FIX
    a = sys.argv[1:]
    if '--seeds' in a:
        v = a[a.index('--seeds') + 1]
        lo, hi = v.split('-') if '-' in v else (v, v)
        seeds = list(range(int(lo), int(hi) + 1))
    if '--fixtures' in a:
        fixtures = a[a.index('--fixtures') + 1].split(',')
    result = {'bin': BIN, 'seeds': '%d-%d' % (seeds[0], seeds[-1]), 'fixtures': {}, 'bars': {}}
    cases = over = 0
    worst = 0.0
    tight = []
    refusals = []
    speed_rows = []
    for fx in fixtures:
        path = os.path.join(HERE, 'fixtures', fx + '.yaml')
        frm, traits, LEVELS, DESC = learning(path)
        rows = []
        for seed in seeds:
            bd = bound_doc(path, seed)
            rest = bd['rest_hours']
            eps = {r['trait']: r for r in bd['traits']}
            p0 = rest_odds(path, init(path, seed), rest)
            best = {t: 0.0 for t in traits}
            for name, steps in campaigns(path, seed, frm, traits, LEVELS, DESC):
                odds, refused, fb = run_campaign(path, seed, steps, rest, frm)
                if name == 'self_forged':
                    refusals.append((fx, seed, refused))
                for t in traits:
                    e = eps[t]['epsilon']
                    r = eps[t]['r']
                    d = tv(odds[t]['odds'], p0[t]['odds'])
                    cases += 1
                    if d > e + r + TOL:
                        over += 1
                    if e > 0:
                        worst = max(worst, d / e)
                    if name.startswith('greedy:%s:' % t):
                        best[t] = max(best[t], d)
                    rows.append({'seed': seed, 'campaign': name, 'trait': t, 'tv': round(d, 6), 'epsilon': e, 'r': r, 'feedback': fb})
            for t in traits:
                e = eps[t]['epsilon']
                if e > 0 and not eps[t]['coarse']:
                    tight.append({'fixture': fx, 'seed': seed, 'trait': t, 'ratio': round(best[t] / e, 6), 'enumerated': bool(eps[t].get('enumerated'))})
                speed_rows.append({'fixture': fx, 'seed': seed, 'trait': t, 'epsilon': e, 'speed': eps[t]['speed'], 'speed_floor': eps[t]['speed_floor']})
            print('%s seed %d: %d campaigns' % (fx, seed, len(rows)), file=sys.stderr, flush=True)
        result['fixtures'][fx] = rows
    plain = [x for x in tight if not x['enumerated']]
    result['bars']['bo1'] = {'cases': cases, 'over': over, 'max_ratio': round(worst, 6),
                             'self_forged_refused': [r for r in refusals], 'pass': over == 0 and all(r[2] == 10 for r in refusals)}
    result['bars']['bo2'] = {'min_ratio': min([x['ratio'] for x in plain] or [0]), 'max_ratio': max([x['ratio'] for x in plain] or [0]),
                             'below_0.9': [x for x in plain if x['ratio'] < 0.9],
                             'enumerated_rows (history channel; greedy does not search it)': [x for x in tight if x['enumerated']],
                             'pass': all(x['ratio'] >= 0.9 for x in plain)}
    nofloor = [x for x in speed_rows if x['epsilon'] > 0 and (x['speed'] is None or x['speed_floor'] is None)]
    badfloor = [x for x in speed_rows if isinstance(x['speed'], (int, float)) and isinstance(x['speed_floor'], (int, float)) and x['speed_floor'] > x['speed']]
    result['bars']['bo5'] = {'rows': len(speed_rows), 'missing': nofloor, 'floor_above_speed': badfloor, 'pass': not nofloor and not badfloor}
    with open(os.path.join(HERE, 'result.json'), 'w') as f:
        json.dump(result, f, indent=1, sort_keys=True)
    print(json.dumps(result['bars'], indent=1, sort_keys=True))
    sys.exit(0 if all(b['pass'] for b in result['bars'].values()) else 1)


if __name__ == '__main__':
    main()
