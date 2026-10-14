// Real-browser monitor regressions, with no npm dependencies. Requires Node 22+
// and Chrome/Chromium. Build `cargo build --release -p probbit-cli`, then run:
//   node probbit-cli/tests/monitor_browser.mjs
// Optional: CHROME_BIN, PROBBIT_BIN, PROBBIT_BROWSER_OUT. Artifacts default to a
// temporary directory, never the source tree. Only synthetic, local data is used.
import {spawn, spawnSync} from 'node:child_process';
import {appendFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
import path from 'node:path';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const bin = process.env.PROBBIT_BIN || path.join(repo, 'target/release', process.platform === 'win32' ? 'probbit.exe' : 'probbit');
const out = process.env.PROBBIT_BROWSER_OUT
  ? path.resolve(process.env.PROBBIT_BROWSER_OUT)
  : mkdtempSync(path.join(tmpdir(), 'probbit-monitor-browser-'));
mkdirSync(out, {recursive: true});
const children = [], sockets = [], servers = [], failures = [];
const report = {screenshots: [], follow: {}, demo: {}, browserErrors: failures};
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const write = (name, text) => writeFileSync(path.join(out, name), text);

async function until(check, description, limit = 25000) {
  const start = Date.now();
  while (Date.now() - start < limit) {
    const value = await check();
    if (value) return value;
    await sleep(50);
  }
  throw Error('Timed out: ' + description);
}

function chromeBinary() {
  const candidates = process.env.CHROME_BIN ? [process.env.CHROME_BIN] : [
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
    'chromium', 'chromium-browser', 'google-chrome', 'google-chrome-stable',
    ...[process.env.PROGRAMFILES, process.env['PROGRAMFILES(X86)'], process.env.LOCALAPPDATA]
      .filter(Boolean).map(root => path.join(root, 'Google/Chrome/Application/chrome.exe')),
  ];
  for (const candidate of candidates) {
    if (spawnSync(candidate, ['--version'], {encoding: 'utf8', timeout: 5000}).status === 0) return candidate;
  }
  throw Error('Chrome/Chromium is required; set CHROME_BIN to its executable.');
}

function command(args, input) {
  const result = spawnSync(bin, args, {
    cwd: repo, encoding: 'utf8', input, timeout: 20000,
    env: {...process.env, PROBBIT_THREADS: '2'},
  });
  assert.equal(result.status, 0, String(result.error || result.stderr));
  return result.stdout;
}

async function server(args, tag) {
  const monitorArgs = args.map(arg => path.isAbsolute(arg) && path.dirname(arg) === out ? path.basename(arg) : arg);
  const child = spawn(bin, ['monitor', ...monitorArgs, '--serve'], {
    cwd: out, env: {...process.env, PROBBIT_THREADS: '2'},
  });
  children.push(child);
  let stdout = '', stderr = '', error;
  child.on('error', value => { error = value; });
  child.stdout.on('data', data => { stdout += data; });
  child.stderr.on('data', data => { stderr += data; });
  servers.push(() => { write(tag + '-stdout.txt', stdout); write(tag + '-stderr.txt', stderr); });
  const url = await until(() => {
    if (error) throw error;
    if (child.exitCode !== null) throw Error('Monitor exited: ' + stderr);
    return stdout.match(/http:\/\/127\.0\.0\.1:\d+\//)?.[0];
  }, tag + ' server');
  return {url, child};
}

async function json(url) {
  const response = await fetch(url, {signal: AbortSignal.timeout(5000)});
  assert.equal(response.status, 200, url);
  return response.json();
}

async function cdp(url) {
  const ws = new WebSocket(url);
  sockets.push(ws);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(Error('DevTools connection timed out')), 10000);
    ws.onopen = () => { clearTimeout(timer); resolve(); };
    ws.onerror = error => { clearTimeout(timer); reject(error); };
  });
  let id = 0;
  const pending = new Map();
  ws.onmessage = event => {
    const message = JSON.parse(event.data);
    if (message.id) {
      const request = pending.get(message.id);
      if (!request) return;
      clearTimeout(request.timer);
      pending.delete(message.id);
      if (message.error) request.reject(Error(JSON.stringify(message.error)));
      else request.resolve(message.result);
    } else if (message.method === 'Runtime.exceptionThrown') {
      failures.push(message.params);
    }
  };
  ws.onclose = () => {
    for (const request of pending.values()) {
      clearTimeout(request.timer);
      request.reject(Error('DevTools connection closed'));
    }
    pending.clear();
  };
  return (method, params = {}) => new Promise((resolve, reject) => {
    const requestId = ++id;
    const timer = setTimeout(() => {
      pending.delete(requestId);
      reject(Error('DevTools timed out: ' + method));
    }, 10000);
    pending.set(requestId, {resolve, reject, timer});
    ws.send(JSON.stringify({id: requestId, method, params}));
  });
}

try {
  assert.equal(typeof WebSocket, 'function', 'Node 22+ is required');
  assert.ok(existsSync(bin), 'Build the release CLI first, or set PROBBIT_BIN');
  const profile = mkdtempSync(path.join(out, 'chrome-profile-'));
  const chrome = spawn(chromeBinary(), [
    '--headless=new', '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
    '--disable-background-networking', '--disable-sync', '--disable-extensions',
    '--no-first-run', '--no-default-browser-check', '--user-data-dir=' + profile, 'about:blank',
  ]);
  children.push(chrome);
  let chromeError, chromeStderr = '';
  chrome.on('error', error => { chromeError = error; });
  chrome.stderr.on('data', data => { chromeStderr += data; });
  chrome.stdout.resume();
  servers.push(() => write('chrome-stderr.txt', chromeStderr));
  await until(() => {
    if (chromeError) throw chromeError;
    if (chrome.exitCode !== null) throw Error('Chrome exited: ' + chromeStderr);
    return existsSync(path.join(profile, 'DevToolsActivePort'));
  }, 'Chrome DevTools');
  const port = readFileSync(path.join(profile, 'DevToolsActivePort'), 'utf8').split('\n')[0];
  const tabs = await json('http://127.0.0.1:' + port + '/json/list');
  const send = await cdp(tabs.find(tab => tab.type === 'page').webSocketDebuggerUrl);
  await send('Page.enable');
  await send('Runtime.enable');
  // Keep a test-only handle to the real SSE connection for the metadata-without-frame boundary.
  // The product page has no test hooks and its normal EventSource behavior is unchanged.
  await send('Page.addScriptToEvaluateOnNewDocument', {source:`{
    const NativeEventSource = window.EventSource;
    window.EventSource = class extends NativeEventSource {
      constructor(...args) { super(...args); window.monitorTestStream = this; }
    };
  }`});
  await send('Page.addScriptToEvaluateOnNewDocument', {source:`{
    const nativeFetch = window.fetch;
    window.monitorFetch = {active:0, maximum:0, docs:[]};
    window.fetch = async (...args) => {
      if (!String(args[0]).startsWith('/doc/')) return nativeFetch(...args);
      const stats = window.monitorFetch; stats.active++; stats.maximum = Math.max(stats.maximum,stats.active); stats.docs.push(args[0]);
      try {
        if (new URL(location.href).searchParams.has('gap') && args[0] === '/doc/2') return new Response('',{status:404});
        return await nativeFetch(...args);
      } finally { stats.active--; }
    };
    document.addEventListener('DOMContentLoaded', () => {
      const label = document.createElement('span'); label.className='synthetic'; label.textContent='SYNTHETIC TEST';
      document.querySelector('.titlebar').insertBefore(label,document.querySelector('.identity'));
      const sourceLabel=document.getElementById('synthetic'),syncLabel=()=>{label.hidden=!sourceLabel.hidden;};
      new MutationObserver(syncLabel).observe(sourceLabel,{attributes:true,attributeFilter:['hidden']});syncLabel();
    });
  }`});
  await send('Emulation.setEmulatedMedia', {features: [{name: 'prefers-reduced-motion', value: 'reduce'}]});
  const value = async expression => {
    const result = await send('Runtime.evaluate', {expression, returnByValue: true, awaitPromise: true});
    assert.ok(!result.exceptionDetails, JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  const viewport = (width, height) => send('Emulation.setDeviceMetricsOverride', {width, height, deviceScaleFactor: 1, mobile: false});
  const navigate = async url => {
    await send('Page.navigate', {url});
    await until(() => value(`location.href === ${JSON.stringify(url)} && document.readyState === 'complete'`), 'monitor page load');
  };
  const state = () => value(`({
    event: document.getElementById('ev')?.textContent,
    identity: document.getElementById('identity')?.textContent,
    note: document.getElementById('note')?.textContent,
    badge: document.getElementById('badge')?.textContent,
    line: document.getElementById('line')?.textContent,
    why: document.getElementById('why')?.textContent,
    saidHidden: document.getElementById('said')?.hidden,
    emptyHidden: document.getElementById('empty')?.hidden,
    rawHidden: document.getElementById('raw')?.hidden,
    stream: document.getElementById('stream-title')?.textContent,
    mode: document.getElementById('stream')?.dataset.mode,
    streamDetail: document.getElementById('stream-detail')?.textContent,
    workspaceHidden: document.getElementById('workspace')?.hidden,
    view: document.getElementById('view-mode')?.textContent,
    historyCount: document.getElementById('history-count')?.textContent,
    historyNote: document.getElementById('history-note')?.textContent,
    historicalNoteHidden: document.getElementById('historical-note')?.hidden,
    learnedHidden: document.getElementById('learned')?.hidden,
    changes: document.getElementById('changes')?.textContent,
    follow: document.getElementById('follow')?.textContent,
    selected: document.getElementById('channel-title')?.textContent,
    channelValue: document.getElementById('channel-value')?.textContent,
    channelOdds: document.getElementById('channel-odds')?.textContent,
    scrollWidth: document.documentElement.scrollWidth, width: innerWidth,
    height: document.documentElement.scrollHeight
  })`);
  async function shot(name, width, height) {
    await viewport(width, height); await sleep(120);
    const frame = await state();
    assert.ok(frame.scrollWidth <= width, 'Horizontal overflow: ' + JSON.stringify(frame));
    assert.ok(!(await value(`document.getElementById('foot').textContent`)).includes(repo) && !(await value(`document.getElementById('foot').textContent`)).includes(out),'Screenshots must not embed private fixture paths');
    const screenshot = await send('Page.captureScreenshot', {
      format:'png', captureBeyondViewport:true,
      clip:{x:0,y:0,width,height:Math.max(height,frame.height),scale:1},
    });
    write(name+'.png',Buffer.from(screenshot.data,'base64'));
    report.screenshots.push({name,...frame});
  }
  const event = number => until(async () => (await state()).event === 'event '+number, 'event '+number);
  const click = selector => value(`document.querySelector(${JSON.stringify(selector)}).click()`);
  const rows = () => value(`[...document.querySelectorAll('#channel-rows tr')].map(r=>({key:r.dataset.channel,name:r.querySelector('button').textContent,selected:r.getAttribute('aria-selected'),current:r.querySelector('.current-col').textContent,odds:r.querySelector('.odds-col').textContent}))`);
  const selected = key => click('#channel-rows button[data-channel="'+key+'"]');

  const demo = await server(['--demo','drives'],'drives');
  await viewport(1280,820); await navigate(demo.url); await event(3);
  report.demo.event3 = await state();
  assert.ok(report.demo.event3.stream.includes('SYNTHETIC'));
  assert.equal((await rows()).length,13);
  await value(`document.getElementById('theme').value='light';document.getElementById('theme').dispatchEvent(new Event('change'))`);
  await selected('drive:want:learn');
  assert.equal((await state()).selected,'learn · wanting');
  assert.equal((await state()).channelOdds,'engine units');
  const doc3=await json(demo.url+'doc/3');
  assert.equal((await state()).channelValue,doc3.drives.want.learn.toFixed(2));
  assert.ok(await value(`document.getElementById('channel-facts').textContent.includes('0 to 3')`));
  assert.equal(await value(`getComputedStyle(document.querySelector('.spark')).transitionDuration`),'0s');
  await shot('activity-desktop-light',1280,820);
  assert.ok((await state()).height<=820,'Useful desktop should fit one viewport');
  await value(`document.getElementById('theme').value='dark'; document.getElementById('theme').dispatchEvent(new Event('change'))`);
  await shot('activity-desktop-dark',1280,820);
  await event(4); await selected('drive:surprise:learn');
  report.demo.negativeError=await state();
  assert.ok(Number((await state()).channelValue)<0);
  assert.ok(await value(`document.getElementById('channel-facts').textContent.includes('Below expectation')`));
  await event(5);
  assert.equal((await state()).selected,'learn · prediction error','Selection survives new event');
  assert.equal((await state()).channelValue,'0.00');
  await until(async()=>(await state()).note.includes('complete; restarting'),'demo completion cue');
  await until(async()=>(await state()).note.includes('loop 2'),'demo loop 2');
  report.demo.restarted=await state();
  assert.ok(['event 0','event 1'].includes(report.demo.restarted.event));
  if(report.demo.restarted.event==='event 0')assert.equal(report.demo.restarted.workspaceHidden,true);

  // Author a different persona: no assumptions about the tutor, a preset character, or fixed mood/trait names.
  const persona=path.join(out,'instrument-fixture.json');
  writeFileSync(persona,JSON.stringify({probbit_persona:1,identity:{name:'Field unit synthetic',version:'1'},
    traits:[{id:'response_depth',levels:['brief','detailed'],say:['be brief','provide detail']},{id:'release_gate',levels:['low','high'],prior:[0.2,0.8],vouch:0.99,hold:'low'}],
    moods:[{id:'reserve',levels:['low','steady','full'],inertia:0.5,half_life_hours:4}],
    inputs:[{id:'boost',kind:'flag',effects:{reserve:2,response_depth:1}}]}));
  const full=path.join(out,'follow-full.strand'),growing=path.join(out,'follow-growing.strand');
  const outputs=command(['live',persona,'--seed','2','--clock','fixed','--strand',full],
    '{"boost":true,"elapsed_hours":1}\n{}\n{"boost":true,"elapsed_hours":2}\n').trim().split('\n').map(JSON.parse);
  const lines=readFileSync(full,'utf8').match(/.*\n/g);writeFileSync(growing,lines.slice(0,2).join(''));
  const follow=await server([growing],'follow');
  await viewport(1280,820);await navigate(follow.url);await event(1);
  assert.equal((await state()).identity,'Field unit synthetic');
  assert.equal(await value(`document.querySelectorAll('#identity synthetic').length`),0,'Identity uses textContent');
  assert.ok((await state()).changes.includes('First recorded event'));
  assert.ok((await rows()).find(r=>r.key==='trait:release_gate').current.includes('held'));
  await selected('trait:release_gate');assert.ok(await value(`document.getElementById('channel-sub').textContent.includes('Held at declared level')`));assert.ok(await value(`document.getElementById('channel-facts').textContent.includes('Before hold')`));
  await click('[data-filter="mood"]');assert.deepEqual((await rows()).map(r=>r.name),['reserve']);
  assert.equal((await state()).selected,'reserve');assert.ok(await value(`document.getElementById('channel-sub').textContent.includes('No previous event')`));
  await click('[data-filter="all"]');
  await click('[data-sort="name"] button');
  assert.deepEqual((await rows()).map(r=>r.name),['release_gate','reserve','response_depth']);
  await click('[data-sort="name"] button');
  assert.deepEqual((await rows()).map(r=>r.name),['response_depth','reserve','release_gate']);
  await value(`document.getElementById('search').value='res';document.getElementById('search').dispatchEvent(new Event('input'))`);
  assert.deepEqual((await rows()).map(r=>r.name),['response_depth','reserve']);
  await value(`document.getElementById('search').value='no-such-channel';document.getElementById('search').dispatchEvent(new Event('input'))`);
  assert.equal((await rows()).length,0);assert.equal(await value(`document.getElementById('inspector').hidden`),true);
  await value(`document.getElementById('search').value='';document.getElementById('search').dispatchEvent(new Event('input'));document.querySelector('#channel-rows button').focus()`);
  await send('Input.dispatchKeyEvent',{type:'keyDown',key:'ArrowDown',code:'ArrowDown',windowsVirtualKeyCode:40});
  await send('Input.dispatchKeyEvent',{type:'keyUp',key:'ArrowDown',code:'ArrowDown',windowsVirtualKeyCode:40});
  assert.equal((await state()).selected,'reserve');
  const firstOdds=outputs[0].mood.reserve.odds[outputs[0].mood.reserve.level];
  assert.equal((await state()).channelOdds,(firstOdds*100).toFixed(1)+'%');
  report.table={sort:true,filter:true,search:true,selection:true,keyboard:true,genericPersona:true,confidenceHold:true};
  appendFileSync(growing,lines[2].slice(0,-1));await sleep(350);assert.equal((await state()).event,'event 1');
  const appended=Date.now();appendFileSync(growing,'\n');await event(2);report.follow.appendLatencyMs=Date.now()-appended;
  assert.ok(report.follow.appendLatencyMs<1000);assert.equal((await state()).selected,'reserve');
  assert.equal((await state()).line,outputs[1].line);assert.deepEqual(await json(follow.url+'doc/2'),outputs[1]);
  await click('#hold');appendFileSync(growing,lines[3]);
  await until(async()=>(await state()).streamDetail.includes('latest received event 3'),'held view receives event');
  assert.equal((await state()).event,'event 2');assert.equal((await state()).line,outputs[1].line);assert.ok((await state()).follow.includes('+1'));
  await click('#previous');assert.equal((await state()).line,outputs[0].line);
  await value(`document.getElementById('scrubber').focus()`);
  await send('Input.dispatchKeyEvent',{type:'keyDown',key:'ArrowRight',code:'ArrowRight',windowsVirtualKeyCode:39});
  await send('Input.dispatchKeyEvent',{type:'keyUp',key:'ArrowRight',code:'ArrowRight',windowsVirtualKeyCode:39});
  assert.equal((await state()).event,'event 2');await click('#follow');await event(3);
  report.follow.heldView=true;
  // Advance only the browser's receiver-age clock. Channel state must not decay on a timer.
  const stable=await value(`document.getElementById('detail-chart').innerHTML`);
  await value(`window.realNow=Date.now;Date.now=()=>window.realNow()+61000`);
  await until(async()=>(await state()).mode==='stale','explicit stale event threshold');
  assert.equal(await value(`document.getElementById('detail-chart').innerHTML`),stable,'Quiet state is not fabricated');
  assert.ok((await state()).streamDetail.includes('does not prove the agent is idle'));
  await value(`Date.now=window.realNow`);report.follow.stale=true;

  // Reload only returns stance documents for history, not original raw clock/learned frames.
  await navigate(follow.url+'?backfill');await event(3);
  await until(async()=>(await state()).historyCount==='3/3 recent documents','history backfill');
  await click('[data-event="1"]');report.follow.documentOnly=await state();
  assert.equal((await state()).line,outputs[0].line);assert.equal((await state()).historicalNoteHidden,false);assert.equal((await state()).learnedHidden,true);
  assert.ok(await value(`document.getElementById('input-facts').textContent.includes('Not captured in this frame')`));
  assert.equal(await value(`document.getElementById('raw').getAttribute('href')`),'/doc/1');
  await shot('activity-narrow-history',390,844);
  await click('#follow');command(['live','control',growing,'pause','--by','human:fixture','--reason','synthetic browser check']);
  await until(async()=>(await state()).mode==='paused','strand pause');assert.ok((await state()).stream.includes('STATE FROZEN'));
  command(['live','control',growing,'retire','--by','human:fixture','--reason','synthetic browser check']);
  await until(async()=>(await state()).mode==='retired','strand retired');assert.ok((await state()).streamDetail.includes('final'));
  report.follow.retired=await state();
  follow.child.kill('SIGTERM');await until(async()=>(await state()).mode==='disconnected','disconnected status');assert.equal((await state()).line,outputs[2].line);
  await shot('activity-disconnected',1280,820);
  await until(()=>follow.child.exitCode!==null||follow.child.signalCode!==null,'server shutdown');
  await server([growing,'--port',new URL(follow.url).port],'reconnected');
  await until(async()=>(await state()).mode==='retired','SSE reconnect');assert.equal((await state()).line,outputs[2].line);

  // A missing event remains a gap, disabled in history and unjoined in the selected channel plot.
  await navigate(follow.url+'?gap');await event(3);
  await until(async()=>(await state()).historyCount==='2/3 recent documents','history gap');
  assert.equal(await value(`document.querySelector('[data-event="2"]').disabled`),true);
  await selected('mood:reserve');
  const paths=await value(`[...document.querySelectorAll('#detail-chart path')].map(p=>p.getAttribute('d'))`);
  assert.ok(paths.length===3&&paths.every(d=>!d.includes('L')&&d.split('M').length===3),'Missing event is not interpolated: '+paths);
  report.historyGap=true;

  const checkpoint=path.join(out,'checkpoint.strand');
  command(['live',persona,'--seed','2','--clock','fixed','--checkpoint-every','2','--strand',checkpoint],'{}\n{}\n{}\n');
  const cp=await server([checkpoint],'checkpoint');await navigate(cp.url);await event(3);
  await until(async()=>(await state()).historyCount==='2/2 recent documents','checkpoint bound');
  assert.ok((await state()).badge.includes('from checkpoint 2'));assert.equal(await value(`document.getElementById('scrubber').min`),'2');report.checkpoint=true;

  const longStrand=path.join(out,'long-history.strand'),longFull=path.join(out,'long-full.strand');
  command(['live',persona,'--seed','2','--clock','fixed','--strand',longFull],'{}\n'.repeat(111));
  const longLines=readFileSync(longFull,'utf8').match(/.*\n/g);writeFileSync(longStrand,longLines.slice(0,61).join(''));
  const longServer=await server([longStrand],'long-history');await navigate(longServer.url);await event(60);
  await until(async()=>(await state()).historyCount==='50/50 recent documents','bounded history');
  assert.equal(await value(`document.querySelectorAll('#history-strip button').length`),50);assert.equal(await value(`document.getElementById('scrubber').min`),'11');
  const fetches=await value(`window.monitorFetch`);assert.ok(fetches.maximum<=4&&fetches.maximum>0);assert.equal(fetches.docs.length,49);report.bounds={documents:50,maximumFetches:fetches.maximum};
  await click('#hold');appendFileSync(longStrand,longLines.slice(61).join(''));
  await until(async()=>(await state()).streamDetail.includes('latest received event 111'),'held outside window');
  await until(async()=>(await state()).historyCount==='50/50 recent documents','new bounded window');
  assert.equal((await state()).event,'event 60');assert.ok((await state()).historyNote.includes('outside the recent window (#62–#111)'));
  assert.equal(await value(`document.getElementById('scrubber').parentNode.hidden`),true);assert.equal(await value(`document.querySelectorAll('#history-strip [aria-current="true"]').length`),0);
  await shot('activity-held-outside-window',360,800);await click('#follow');await event(111);
  assert.equal(await value(`document.getElementById('scrubber').parentNode.hidden`),false);
  assert.ok(await value(`performance.getEntriesByType('resource').every(r=>new URL(r.name).origin===location.origin)`),'No external resources');
  // Persona changes without an accompanying frame must erase prior visible state, including held views.
  await value(`document.getElementById('hold').click();window.monitorTestStream.close();window.monitorTestStream.dispatchEvent(new MessageEvent('meta',{data:JSON.stringify({persona:{name:'Second fixture',version:'1',seed:9,individual:'synthetic-second'},path:'synthetic.strand',traits:[],moods:[],inputs:[],history:[]})}));`);
  report.metadataOnly=await state();assert.equal(report.metadataOnly.identity,'Second fixture');assert.equal(report.metadataOnly.line,'');assert.equal(report.metadataOnly.why,'');assert.equal(report.metadataOnly.workspaceHidden,true);assert.equal(report.metadataOnly.rawHidden,true);assert.equal(report.metadataOnly.learnedHidden,true);assert.equal(report.metadataOnly.stream,'AWAITING FIRST EVENT');
  await value(`window.monitorTestStream.dispatchEvent(new Event('error'))`);assert.equal((await state()).mode,'disconnected');assert.equal((await state()).workspaceHidden,true);
  // No moods or drives is a normal supported persona, not an empty app.
  const bare=path.join(out,'traits-only.json'),bareStrand=path.join(out,'traits-only.strand');
  writeFileSync(bare,JSON.stringify({probbit_persona:1,identity:{name:'Minimal fixture',version:'1'},traits:[{id:'approach',levels:['consider','act']}]}));
  command(['live',bare,'--clock','fixed','--strand',bareStrand],'{}\n');
  const bareServer=await server([bareStrand],'traits-only');await navigate(bareServer.url);await event(1);assert.equal((await rows()).length,1);assert.equal((await state()).selected,'approach');
  await click('[data-filter="drive"]');assert.equal((await rows()).length,0);assert.equal(await value(`document.getElementById('no-match').hidden`),false);report.traitsOnly=true;
  const refusing=path.join(out,'unreleased.json'),refusingStrand=path.join(out,'unreleased.strand');
  const refusingPersona=JSON.parse(readFileSync(path.join(repo,'examples/persona/tutor.json'),'utf8'));
  refusingPersona.engine={op:'sample',sweeps:8,on_conflict:'yield'};writeFileSync(refusing,JSON.stringify(refusingPersona));
  const refused=JSON.parse(command(['live',refusing,'--clock','fixed','--strand',refusingStrand],'{}\n').trim());
  const unreleased=Object.keys(refused.stance).find(k=>refused.stance[k].released===false);assert.ok(unreleased,'Fixture must genuinely withhold a trait');
  const refusingServer=await server([refusingStrand],'unreleased');await navigate(refusingServer.url);await event(1);await selected('trait:'+unreleased);
  assert.ok((await rows()).find(r=>r.key==='trait:'+unreleased).current.includes('unreleased'));assert.ok(await value(`document.getElementById('channel-sub').textContent.includes('engine did not vouch')`));report.unreleased=true;
  const capped=path.join(out,'declared-scales.json'),cappedStrand=path.join(out,'declared-scales.strand');
  writeFileSync(capped,JSON.stringify({probbit_persona:1,identity:{name:'Scale fixture',version:'1'},traits:[{id:'approach',levels:['consider','act']}],drives:{goals:[{id:'repair'},{id:'explore'}],wanting:{cap:7},afterglow:{cap:4},expectation:{pe_cap:2.5}}}));
  command(['live',capped,'--clock','fixed','--strand',cappedStrand],'{}\n');
  const cappedServer=await server([cappedStrand],'declared-scales');await navigate(cappedServer.url);await event(1);
  await selected('drive:surprise:repair');assert.ok(await value(`document.getElementById('channel-facts').textContent.includes('-2.5 to 2.5')`));
  assert.ok(await value(`document.getElementById('detail-chart').textContent.includes('2.50')`));report.declaredScales=true;
  assert.equal(failures.length,0,JSON.stringify(failures));report.ok=true;
} catch (error) {
  report.error = String(error.stack || error);
  process.exitCode = 1;
} finally {
  for (const save of servers) save();
  write('browser-report.json', JSON.stringify(report, null, 2) + '\n');
  for (const ws of sockets) ws.close();
  for (const child of children) child.kill('SIGTERM');
  console.log(JSON.stringify({artifacts: out, ...report}, null, 2));
}
