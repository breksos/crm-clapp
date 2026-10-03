// Shared data + view-model for all three Breksos directions.
export const ME = 'u1', MY_AGENT = 'a1';
const users = [
  { id: 'u1', name: 'Priya Shah', ini: 'PS', role: 'Account Executive', hue: 40, online: true, access: 'Member' },
  { id: 'u2', name: 'Marcus Oyelaran', ini: 'MO', role: 'Account Executive', hue: 255, online: true, access: 'Member' },
  { id: 'u3', name: 'Lena Fischer', ini: 'LF', role: 'Senior Account Executive', hue: 155, online: true, access: 'Member' },
  { id: 'u4', name: 'Tomás Aguilar', ini: 'TA', role: 'SDR', hue: 320, online: false, access: 'Member' },
  { id: 'u5', name: 'Hana Kobayashi', ini: 'HK', role: 'Account Executive', hue: 200, online: true, access: 'Member' },
  { id: 'u6', name: 'Daniel Brooks', ini: 'DB', role: 'Sales Manager', hue: 95, online: false, access: 'Admin' },
  { id: 'u7', name: 'Sofia Marchetti', ini: 'SM', role: 'Account Executive', hue: 10, online: false, access: 'Manager' },
  { id: 'u8', name: 'Kwame Mensah', ini: 'KM', role: 'SDR', hue: 125, online: true, access: 'Member' },
];
const agents = [
  { id: 'a1', name: 'Nova', ini: 'NV', owner: 'u1', status: 'running', doing: 'Drafting security answers · Northwind', perm: 'Up to Negotiation · emails need approval over 5% discount' },
  { id: 'a2', name: 'Relay', ini: 'RL', owner: 'u2', status: 'running', doing: 'Rebuilding Halvorsen close plan', perm: 'Up to Proposal · may send emails' },
  { id: 'a3', name: 'Quill', ini: 'QL', owner: 'u3', status: 'idle', doing: 'Idle · last run 25m ago', perm: 'Notes and summaries only' },
  { id: 'a4', name: 'Scout', ini: 'SC', owner: 'u6', team: true, status: 'running', doing: 'Enriching 14 inbound leads', perm: 'Create deals in Qualify · enrich records' },
];
const UM = {}, AM = {};
users.forEach(u => UM[u.id] = u); agents.forEach(a => AM[a.id] = a);
const first = n => n.split(' ')[0];
export function actor(id) {
  const u = UM[id];
  if (u) return { id, agent: false, human: true, name: u.name, first: first(u.name), ini: u.ini, hue: u.hue, online: u.online, role: u.role, of: '', label: u.name, access: u.access, isMe: id === ME };
  const a = AM[id], o = UM[a.owner];
  const of = a.team ? 'Team agent' : `${first(o.name)}'s agent`;
  return { id, agent: true, human: false, name: a.name, first: a.name, ini: a.ini, hue: o.hue, online: a.status === 'running', running: a.status === 'running', role: of, of, ownerName: o.name, ownerFirst: first(o.name), ownerIni: o.ini, label: `${a.name} · ${of}`, doing: a.doing, perm: a.perm, isMine: a.owner === ME };
}
export const stages = [
  { id: 'qualify', name: 'Qualify', prob: 10 }, { id: 'discovery', name: 'Discovery', prob: 25 },
  { id: 'proposal', name: 'Proposal', prob: 50 }, { id: 'negotiation', name: 'Negotiation', prob: 75 },
  { id: 'won', name: 'Won', prob: 100 }, { id: 'lost', name: 'Lost', prob: 0 },
];
const SM = {}; stages.forEach((s, i) => { SM[s.id] = s; s.i = i; });
const companies = [
  ['c1', 'Northwind Freight & Cold Storage Logistics', 'northwindfreight.com', 'Logistics', 'Memphis, US', '2,400', 'u1'],
  ['c2', 'Halvorsen Maritime Insurance Group', 'halvorsen.no', 'Insurance', 'Bergen, NO', '860', 'u2'],
  ['c3', 'Kestrel Biotech', 'kestrelbio.de', 'Biotech', 'Heidelberg, DE', '310', 'u3'],
  ['c4', 'Ostara Renewable Energy Cooperative of Bavaria', 'ostara-eg.de', 'Energy', 'Regensburg, DE', '140', 'u3'],
  ['c5', 'Meridian Health Partners', 'meridianhp.com', 'Healthcare', 'Denver, US', '5,100', 'u1'],
  ['c6', 'Tanaka Precision Components', 'tanaka-pc.co.jp', 'Manufacturing', 'Nagoya, JP', '1,900', 'u5'],
  ['c7', 'Bluebird Municipal Employees Credit Union', 'bluebirdcu.org', 'Financial services', 'Columbus, US', '420', 'u7'],
  ['c8', 'Arclight Studios', 'arclight.studio', 'Media', 'Vancouver, CA', '230', 'u2'],
  ['c9', 'Fenwick & Rowe Architecture Partnership', 'fenwickrowe.co.uk', 'Architecture', 'London, UK', '120', 'u7'],
  ['c10', 'Solano Valley Agricultural Supply Company', 'solanoag.com', 'Agriculture', 'Fresno, US', '650', 'u8'],
  ['c11', 'Vireo Labs', 'vireolabs.io', 'Life sciences', 'Boston, US', '95', 'u5'],
  ['c12', 'Castellan Hotels & Resorts International', 'castellan.com', 'Hospitality', 'Lisbon, PT', '12,000', 'u2'],
  ['c13', 'Pinecrest Unified School District 14', 'pinecrest14.edu', 'Education', 'Boise, US', '1,100', 'u7'],
  ['c14', 'Umbra Security', 'umbrasec.com', 'Security', 'Austin, US', '180', 'u8'],
].map(([id, name, domain, industry, hq, size, owner]) => ({ id, name, domain, industry, hq, size, owner }));
const CM = {}; companies.forEach(c => CM[c.id] = c);
const contacts = [
  ['p1', 'Grace Liu', 'VP Operations', 'c1', 'u1', '6m', 'a1'], ['p2', 'Daniel Okafor', 'Chief Financial Officer', 'c1', 'u1', '3d', 'u1'],
  ['p3', 'Mei Tan', 'Head of IT Security', 'c1', 'u1', '1h', 'a1'], ['p4', 'Oliver Grant', 'Head of Broking', 'c2', 'u2', '12m', 'u2'],
  ['p5', 'Dr. Anika Rao', 'Director of Lab Informatics', 'c3', 'u3', '1h', 'u3'], ['p6', 'Jonas Weber', 'Managing Board Member', 'c4', 'u3', '5h', 'a3'],
  ['p7', 'Carmen Díaz', 'Chief Operating Officer', 'c5', 'u1', '40m', 'u1'], ['p8', 'Kenji Watanabe', 'Plant Quality Manager', 'c6', 'u5', '3h', 'a4'],
  ['p9', 'Rachel Kim', 'VP Member Experience', 'c7', 'u4', '5h', 'u4'], ['p10', 'Theo Park', 'Head of Infrastructure', 'c8', 'u2', '1d', 'u2'],
  ['p11', 'Amelia Rowe', 'Partner', 'c9', 'u7', '4h', 'a4'], ['p12', 'Luis Ortega', 'Supply Chain Director', 'c10', 'u8', '14d', 'u8'],
  ['p13', 'Sarah Chen', 'VP Research Operations', 'c11', 'u5', '2h', 'u5'], ['p14', 'Inês Carvalho', 'Group Director of Digital', 'c12', 'u2', '2h', 'a2'],
  ['p15', 'Mark Hollis', 'Director of Technology', 'c13', 'u7', '1d', 'u7'], ['p16', 'Nadia Hassan', 'Chief Information Security Officer', 'c14', 'u8', '13d', 'u8'],
].map(([id, name, title, co, owner, t, by]) => ({ id, name, title, co, owner, t, by, email: name.toLowerCase().replace('dr. ', '').replace(/[^a-z ]/g, '').replace(' ', '.') + '@' + CM[co].domain }));
const PM = {}; contacts.forEach(p => PM[p.id] = p);
const deals = [
  ['d1', 'Fleet telematics rollout — 3 regional depots', 'c1', ['p1', 'p2', 'p3'], 184000, 'USD', 'proposal', 'u1', 'Oct 30', 'a1', '6m', 'Security questionnaire', 'Fri 25 Sep'],
  ['d2', 'Marine cargo policy renewal FY27', 'c2', ['p4'], 96500, 'GBP', 'negotiation', 'u2', 'Oct 12', 'a2', '14m', 'Review broker terms', 'Today'],
  ['d3', 'LIMS integration pilot, two sites', 'c3', ['p5'], 42000, 'EUR', 'discovery', 'u3', 'Nov 20', 'u3', '1h', 'Pilot scope sign-off', 'Oct 2'],
  ['d4', 'Community solar monitoring platform', 'c4', ['p6'], 310000, 'EUR', 'proposal', 'u3', 'Dec 5', 'a3', '5h', 'Grid FAQ follow-up', 'Mon 28 Sep'],
  ['d5', 'Patient intake automation — phase 2', 'c5', ['p7'], 128000, 'USD', 'negotiation', 'u1', 'Oct 8', 'u1', '40m', 'Chase signed MSA', 'Yesterday'],
  ['d6', 'Precision QA vision system, Line 4', 'c6', ['p8'], 18500000, 'JPY', 'discovery', 'u5', 'Nov 28', 'a4', '3h', 'Book plant demo', 'Oct 6'],
  ['d7', 'Member onboarding & KYC modernisation', 'c7', ['p9'], 74000, 'USD', 'qualify', 'u4', 'Dec 15', 'u4', '5h', 'Second call attempt', 'Thu 24 Sep'],
  ['d8', 'Render farm capacity expansion', 'c8', ['p10'], 52000, 'USD', 'won', 'u2', 'Sep 18', 'u2', '1d', '', ''],
  ['d9', 'Practice-wide project management suite', 'c9', ['p11'], 38000, 'GBP', 'qualify', 'u7', 'Jan 10', 'a4', '4h', 'Discovery call', 'Oct 1'],
  ['d10', 'Seasonal inventory forecasting', 'c10', ['p12'], 61000, 'USD', 'discovery', 'u8', 'Nov 14', 'a4', '1d', 'Re-engage Luis Ortega', 'Today'],
  ['d11', 'Assay data platform — enterprise licence', 'c11', ['p13'], 220000, 'USD', 'negotiation', 'u5', 'Oct 20', 'u5', '2h', 'Legal review kickoff', 'Mon 28 Sep'],
  ['d12', 'Group-wide guest messaging platform', 'c12', ['p14'], 405000, 'EUR', 'proposal', 'u2', 'Nov 30', 'a2', '2h', 'Pricing workshop', 'Oct 5'],
  ['d13', 'District device management, 14 schools', 'c13', ['p15'], 88000, 'USD', 'won', 'u7', 'Sep 15', 'u7', '1d', '', ''],
  ['d14', 'SOC tooling consolidation', 'c14', ['p16'], 57000, 'USD', 'lost', 'u8', 'Sep 10', 'u8', '1d', '', ''],
  ['d15', 'Cold chain sensor retrofit — Rotterdam', 'c1', ['p1'], 146000, 'EUR', 'qualify', 'u1', 'Jan 22', 'a1', '1d', 'Intro to EU ops lead', 'Oct 7'],
  ['d16', 'Reinsurance analytics add-on', 'c2', ['p4'], 44000, 'GBP', 'discovery', 'u2', 'Dec 1', 'u2', '2d', 'Data sample request', 'Oct 3'],
  ['d17', 'Lab equipment telemetry', 'c3', ['p5'], 29000, 'EUR', 'lost', 'u3', 'Sep 4', 'u3', '6d', '', ''],
  ['d18', 'Tokyo plant expansion — inline sensors', 'c6', ['p8'], 42000000, 'JPY', 'proposal', 'u5', 'Dec 12', 'u5', '1d', 'Send proposal v2', 'Wed 30 Sep'],
  ['d19', 'Loan officer workflow pilot', 'c7', ['p9'], 36000, 'USD', 'negotiation', 'u7', 'Oct 15', 'a4', '6h', 'Final pricing', 'Fri 25 Sep'],
  ['d20', 'Studio asset pipeline renewal', 'c8', ['p10'], 67000, 'USD', 'qualify', 'u2', 'Feb 2', 'u2', '3d', 'Usage review', 'Oct 9'],
  ['d21', 'Resort booking engine replacement', 'c12', ['p14'], 190000, 'EUR', 'won', 'u3', 'Sep 12', 'u3', '11d', '', ''],
  ['d22', 'Warehouse labour planning module', 'c10', ['p12'], 72000, 'USD', 'discovery', 'u1', 'Nov 26', 'a1', '3h', 'Workshop agenda', 'Tue 29 Sep'],
  ['d23', 'Clinical scheduling integration', 'c5', ['p7'], 54000, 'USD', 'qualify', 'u1', 'Jan 15', 'u1', '2d', 'Qualify budget owner', 'Oct 5'],
].map(([id, title, co, cts, value, cur, stage, owner, close, by, t, next, nextDue]) => ({ id, title, co, cts, value, cur, stage, owner, close, by, t, next, nextDue }));
const DM = {}; deals.forEach(d => DM[d.id] = d);
const activities = [
  ['a1', 'email', 'd1', 'Sent revised pricing for 3 depots to Grace Liu', '6m', 'Today'],
  ['u2', 'call', 'd2', 'Call with Oliver Grant, 24 min — broker terms agreed in principle', '12m', 'Today'],
  ['a2', 'field', 'd2', 'Changed close date Oct 18 → Oct 12', '14m', 'Today'],
  ['a3', 'note', 'd4', 'Summarised board minutes: Q4 budget approved', '25m', 'Today'],
  ['u1', 'meeting', 'd5', 'Demo with Meridian operations team, 6 attendees', '40m', 'Today'],
  ['u3', 'email', 'd3', 'Shared pilot scope document with Dr. Anika Rao', '1h', 'Today'],
  ['a1', 'task', 'd1', 'Drafted answers to 38 of 42 security questions', '1h', 'Today'],
  ['u5', 'call', 'd11', 'Procurement call — legal review starts Monday', '2h', 'Today'],
  ['a2', 'stage', 'd12', 'Moved Discovery → Proposal', '2h', 'Today'],
  ['u1', 'call', 'd1', 'Call with Grace Liu, 18 min — wants depot-level reporting', '3h', 'Today'],
  ['a4', 'create', 'd6', 'Created deal from inbound RFQ', '3h', 'Today'],
  ['u6', 'note', 'd11', 'Approved 12% discount ceiling', '3h', 'Today'],
  ['a4', 'field', 'd9', 'Enriched company: 120 employees, London', '4h', 'Today'],
  ['u4', 'call', 'd7', 'Left voicemail for Rachel Kim', '5h', 'Today'],
  ['a3', 'email', 'd4', 'Replied to Jonas Weber with grid-connection FAQ', '5h', 'Today'],
  ['u2', 'stage', 'd8', 'Marked Won — $52,000', '1d', 'Yesterday'],
  ['a1', 'meeting', 'd1', 'Booked Memphis depot site visit for Oct 2', '1d', 'Yesterday'],
  ['u7', 'stage', 'd13', 'Marked Won — $88,000', '1d', 'Yesterday'],
  ['u8', 'stage', 'd14', 'Marked Lost — stayed with incumbent', '1d', 'Yesterday'],
  ['a4', 'note', 'd10', 'Flagged: no reply from Luis Ortega in 14 days', '1d', 'Yesterday'],
  ['u1', 'note', 'd1', 'Daniel Okafor signs; budget sits in FY26 capex', '1d', 'Yesterday'],
  ['u2', 'note', 'd1', 'Halvorsen used the same telematics vendor — happy to intro', '1d', 'Yesterday'],
  ['a1', 'stage', 'd1', 'Moved Discovery → Proposal', 'Sep 16', 'Earlier'],
  ['u1', 'create', 'd1', 'Created deal', 'Aug 28', 'Earlier'],
].map(([actor, type, deal, text, t, day], i) => ({ id: 'e' + i, actor, type, deal, text, t, day }));
const tasks = [
  ['t1', 'Send security questionnaire answers', 'd1', 'a1', 'Fri 25 Sep', false],
  ['t2', 'Get legal contact at Northwind', 'd1', 'u1', 'Today', false],
  ['t3', 'Confirm Memphis site visit logistics', 'd1', 'u1', 'Thu 1 Oct', false],
  ['t4', 'Build depot ROI model', 'd1', 'a1', 'Sep 21', true],
  ['t5', 'Intro to Halvorsen fleet contact', 'd1', 'u2', 'Sep 22', true],
  ['t6', 'Chase signed MSA from Meridian', 'd5', 'u1', 'Yesterday', false],
  ['t7', 'Review Halvorsen broker terms', 'd2', 'u2', 'Today', false],
  ['t8', 'Workshop agenda for Solano', 'd22', 'a1', 'Tue 29 Sep', false],
  ['t9', 'Book Tanaka plant demo', 'd6', 'u5', 'Tue 6 Oct', false],
].map(([id, title, deal, who, due, done]) => ({ id, title, deal, who, due, done, overdue: due === 'Yesterday', today: due === 'Today' }));
const notes = [
  ['u1', 'd1', 'Yesterday · 16:40', 'Daniel Okafor is the signer. Budget sits in FY26 capex, so anything after Nov 15 slips a year. Grace is the champion but needs depot-level reporting to sell it internally.'],
  ['a1', 'd1', 'Today · 09:12', 'Summary of Mei Tan\u2019s security call: SOC 2 Type II accepted, data residency must be US-only, SSO via Okta. Four questionnaire items need Priya\u2019s answer (items 12, 19, 33, 40).'],
  ['u2', 'd1', 'Yesterday · 11:05', 'Halvorsen ran the same telematics vendor in 2024. Their fleet lead can speak to rollout pain — happy to intro.'],
].map(([actor, deal, t, body], i) => ({ id: 'n' + i, actor, deal, t, body }));

const CUR = ['USD', 'EUR', 'GBP', 'JPY'];
export function money(v, cur, compact) {
  return new Intl.NumberFormat('en-US', { style: 'currency', currency: cur, notation: compact ? 'compact' : 'standard', maximumFractionDigits: compact ? 1 : 0 }).format(v);
}
function sums(list, weighted, stageOf) {
  const g = {};
  list.forEach(d => { const w = weighted ? SM[stageOf(d)].prob / 100 : 1; g[d.cur] = g[d.cur] || { cur: d.cur, total: 0, count: 0 }; g[d.cur].total += d.value * w; g[d.cur].count++; });
  return CUR.filter(c => g[c]).map(c => ({ ...g[c], text: money(g[c].total, c), compact: money(g[c].total, c, true) }));
}
const MO = { Aug: 8, Sep: 9, Oct: 10, Nov: 11, Dec: 12, Jan: 13, Feb: 14 };
const closeKey = c => { const [m, dd] = c.split(' '); return MO[m] * 100 + +dd; };
const TYPE = { call: 'Call', email: 'Email', meeting: 'Meeting', note: 'Note', stage: 'Stage', task: 'Task', field: 'Update', create: 'Created' };
const teamStats = {
  u1: { wr: 42, act: 212, ag: 388, won: '$212K · €48K' }, u2: { wr: 38, act: 244, ag: 301, won: '$52K · £61K' },
  u3: { wr: 51, act: 176, ag: 120, won: '€236K' }, u4: { wr: 0, act: 402, ag: 0, won: '—' },
  u5: { wr: 35, act: 158, ag: 0, won: '¥12M · $40K' }, u6: { wr: 0, act: 64, ag: 510, won: '—' },
  u7: { wr: 44, act: 190, ag: 0, won: '$88K · £22K' }, u8: { wr: 22, act: 355, ag: 0, won: '$18K' },
};
const conversion = [['Qualify', 'Discovery', 62], ['Discovery', 'Proposal', 48], ['Proposal', 'Negotiation', 57], ['Negotiation', 'Won', 44]];
const volume = [[92, 40], [110, 52], [98, 61], [121, 70], [104, 88], [117, 96], [99, 118], [126, 131], [112, 140], [108, 162], [131, 171], [96, 158]];
const fields = [
  ['Deal source', 'Select', 'Deal', 'Inbound, Outbound, Partner, Event'], ['Contract term', 'Number (months)', 'Deal', 'Required at Proposal'],
  ['Champion', 'Contact reference', 'Deal', 'One contact'], ['Procurement portal', 'URL', 'Company', ''],
  ['Agents may contact', 'Checkbox', 'Contact', 'Default off'], ['Region', 'Select', 'Company', 'AMER, EMEA, APAC'],
].map(([name, type, obj, note]) => ({ name, type, obj, note }));
const views = [
  ['open', 'All open deals', 'team', ['Status is Open']], ['mine', 'My open deals', 'me', ['Owner is me or my agent', 'Status is Open']],
  ['closing', 'Closing by Oct 31', 'team', ['Status is Open', 'Close date ≤ Oct 31']], ['eur', 'EUR pipeline', 'me', ['Currency is EUR', 'Status is Open']],
  ['agents', 'Agent-touched today', 'team', ['Last actor is an agent', 'Updated today']], ['stalled', 'Stalled 60+ days', 'me', ['Status is Open', 'No activity in 60 days']],
  ['closed', 'Won & lost, Q3', 'team', ['Status is Won or Lost']],
].map(([id, name, who, filters]) => ({ id, name, who, filters }));

export const initialState = { mode: 'light', page: 'home', phase: 0, ov: {}, own: {}, userActs: [], over: null, flash: null, toast: null, liveDismissed: false, scope: 'team', view: 'open', sort: { key: 'close', dir: 1 }, filterOpen: false, rec: 'd1', tab: 'overview', ptab: 'companies', co: 'c1', ct: 'p1', feedActor: 'all', feedType: 'all', search: false, q: 'north', assignOpen: false, set: 'users', dismissed: {} };

export function start(self) {
  self._key = e => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); self.setState({ search: !self.state.search }); }
    if (e.key === 'Escape') self.setState({ search: false, filterOpen: false, assignOpen: false });
  };
  window.addEventListener('keydown', self._key);
  replay(self);
}
export function stop(self) { window.removeEventListener('keydown', self._key); clearTimeout(self._t); clearTimeout(self._t2); }
function replay(self) {
  clearTimeout(self._t); clearTimeout(self._t2);
  const ov = { ...self.state.ov }; delete ov.d1;
  self.setState({ phase: 0, ov, liveDismissed: false, toast: null });
  self._t = setTimeout(() => self.setState({ phase: 1 }), 3800);
  self._t2 = setTimeout(() => self.setState({ liveDismissed: true }), 12000);
}

export function vals(self, o) {
  const s = self.state, set = p => self.setState(p);
  const av = id => { const a = actor(id); return Object.assign(a, o.av(a)); };
  const live = s.phase >= 1 && s.ov.d1 === undefined;
  const stageOf = d => s.ov[d.id] ?? (d.id === 'd1' ? (live ? 'negotiation' : 'proposal') : d.stage);
  const ownerOf = d => s.own[d.id] ?? d.owner;
  const isOpen = d => !['won', 'lost'].includes(stageOf(d));
  const mineF = d => ownerOf(d) === ME;
  const scoped = deals.filter(d => s.scope === 'team' || mineF(d));
  const go = page => () => { set({ page, search: false, filterOpen: false }); if (page === 'board') replay(self); };
  const openDeal = id => () => set({ page: 'record', rec: id, tab: 'overview', search: false, assignOpen: false });
  const liveAct = live ? [{ id: 'live', actor: 'a1', type: 'stage', deal: 'd1', text: 'Moved Proposal → Negotiation — MSA redlines received from Northwind legal', t: 'just now', day: 'Today', live: true }] : [];
  const allActs = [...s.userActs, ...liveAct, ...activities];
  const act = e => { const d = DM[e.deal]; return { ...e, a: av(e.actor), typeLabel: TYPE[e.type], typeColor: (o.type || {})[e.type], dealTitle: d.title, company: CM[d.co].name, open: openDeal(e.deal) }; };
  const dv = d => {
    const st = stageOf(d), c = CM[d.co], byAgentLive = d.id === 'd1' && live;
    return {
      ...d, stage: st, stageName: SM[st].name, stageColor: o.stage[st], prob: SM[st].prob, company: c.name, coId: c.id,
      owner: av(ownerOf(d)), by: av(byAgentLive ? 'a1' : d.by), t: byAgentLive ? 'just now' : d.t,
      val: money(d.value, d.cur), valC: money(d.value, d.cur, true), open: openDeal(d.id), isOpen: isOpen(d),
      working: d.id === 'd1' && s.phase === 0 && s.ov.d1 === undefined, workingText: 'Nova is reading MSA redlines…',
      justMoved: byAgentLive && !s.liveDismissed, flash: s.flash === d.id,
      hasNext: !!d.next && isOpen(d), overdue: d.nextDue === 'Yesterday', dueToday: d.nextDue === 'Today',
      contactNames: d.cts.map(p => PM[p].name).join(', '), nContacts: d.cts.length,
      onDragStart: e => { self._drag = d.id; try { e.dataTransfer.setData('text/plain', d.id); } catch (x) {} },
      mine: ownerOf(d) === ME,
    };
  };
  const drop = to => {
    const id = self._drag; self._drag = null;
    if (!id) return set({ over: null });
    const d = DM[id], from = stageOf(d);
    if (from === to) return set({ over: null });
    set({ over: null, flash: id, ov: { ...s.ov, [id]: to }, toast: { deal: id, from, to }, liveDismissed: true,
      userActs: [{ id: 'u' + Date.now(), actor: ME, type: 'stage', deal: id, text: `Moved ${SM[from].name} → ${SM[to].name}`, t: 'just now', day: 'Today' }, ...s.userActs] });
    clearTimeout(self._ft); self._ft = setTimeout(() => self.setState({ flash: null, toast: null }), 5000);
  };
  // board
  const columns = stages.map(st => {
    const list = scoped.filter(d => stageOf(d) === st.id);
    return { ...st, color: o.stage[st.id], closed: st.id === 'won' || st.id === 'lost', deals: list.map(dv), count: list.length, empty: list.length === 0,
      totals: sums(list, false, stageOf).map(x => x.compact), totalsText: sums(list, false, stageOf).map(x => x.compact).join('  ·  ') || '—',
      over: s.over === st.id,
      onDragOver: e => { e.preventDefault(); if (s.over !== st.id) set({ over: st.id }); },
      onDragLeave: () => {}, onDrop: e => { e.preventDefault(); drop(st.id); } };
  });
  let toast = null;
  if (s.toast) { const d = DM[s.toast.deal]; toast = { show: true, a: av(ME), verb: 'You moved', deal: d.title, from: SM[s.toast.from].name, to: SM[s.toast.to].name, reason: '', undo: () => { const ov = { ...s.ov }; ov[s.toast.deal] = s.toast.from; set({ ov, toast: null, userActs: s.userActs.slice(1) }); }, dismiss: () => set({ toast: null }) }; }
  else if (live && !s.liveDismissed) toast = { show: true, a: av('a1'), verb: 'moved', deal: DM.d1.title, from: 'Proposal', to: 'Negotiation', reason: 'MSA redlines received from Northwind legal', undo: () => set({ ov: { ...s.ov, d1: 'proposal' }, liveDismissed: true, userActs: [{ id: 'u' + Date.now(), actor: ME, type: 'stage', deal: 'd1', text: 'Reverted Nova\u2019s move: Negotiation → Proposal', t: 'just now', day: 'Today' }, ...s.userActs] }), dismiss: () => set({ liveDismissed: true }) };
  // table
  const vw = views.find(v => v.id === s.view);
  const vf = {
    open: isOpen, mine: d => isOpen(d) && mineF(d), closing: d => isOpen(d) && closeKey(d.close) <= 1031,
    eur: d => isOpen(d) && d.cur === 'EUR', agents: d => AM[d.id === 'd1' && live ? 'a1' : d.by] && /m|h|now/.test(d.t), stalled: () => false,
    closed: d => !isOpen(d),
  }[s.view];
  const sk = s.sort.key, dir = s.sort.dir;
  const keyF = { title: d => d.title, company: d => CM[d.co].name, stage: d => SM[stageOf(d)].i, value: d => CUR.indexOf(d.cur) * 1e12 + d.value, owner: d => actor(ownerOf(d)).name, close: d => closeKey(d.close) }[sk];
  const rowsRaw = deals.filter(d => (s.scope === 'team' || mineF(d)) && vf(d)).sort((a, b) => { const x = keyF(a), y = keyF(b); return (x > y ? 1 : x < y ? -1 : 0) * dir; });
  const sortOn = {}, arrow = {};
  ['title', 'company', 'stage', 'value', 'owner', 'close'].forEach(k => { sortOn[k] = () => set({ sort: { key: k, dir: sk === k ? -dir : 1 } }); arrow[k] = sk === k ? (dir > 0 ? '↑' : '↓') : ''; });
  // record
  const rd = DM[s.rec], rec = dv(rd), rc = CM[rd.co];
  const recActs = allActs.filter(e => e.deal === rd.id).map(act);
  const recTasks = tasks.filter(t => t.deal === rd.id).map(t => ({ ...t, a: av(t.who) }));
  const recNotes = notes.filter(n => n.deal === rd.id).map(n => ({ ...n, a: av(n.actor) }));
  const viewers = (rd.id === 'd1' ? ['u2', 'a1'] : ['u6']).map(av);
  const tabDefs = [['overview', 'Overview', ''], ['activity', 'Activity', recActs.length], ['notes', 'Notes', recNotes.length], ['tasks', 'Tasks', recTasks.filter(t => !t.done).length], ['related', 'Related', '']];
  const related = deals.filter(d => d.co === rd.co && d.id !== rd.id).map(dv);
  const stagePath = stages.slice(0, 4).map(st => ({ ...st, done: SM[rec.stage].i > st.i && rec.isOpen, current: rec.stage === st.id, color: o.stage[st.id] }));
  // people
  const coDeals = c => deals.filter(d => d.co === c.id);
  const coRows = companies.map(c => { const dl = coDeals(c).filter(isOpen); const last = activities.find(e => DM[e.deal].co === c.id); return { ...c, owner: av(c.owner), openCount: dl.length, openText: sums(dl, false, stageOf).map(x => x.compact).join(' · ') || '—', nContacts: contacts.filter(p => p.co === c.id).length, last: last ? act(last) : null, lastT: last ? last.t : '—', lastBy: last ? av(last.actor) : av(c.owner), active: s.co === c.id, select: () => set({ co: c.id }) }; });
  const ctRows = contacts.map(p => ({ ...p, company: CM[p.co].name, owner: av(p.owner), lastBy: av(p.by), active: s.ct === p.id, select: () => set({ ct: p.id }) }));
  const sc = CM[s.co], sp = PM[s.ct];
  const company = { ...sc, owner: av(sc.owner), contacts: contacts.filter(p => p.co === sc.id).map(p => ({ ...p, lastBy: av(p.by) })), deals: coDeals(sc).map(dv), acts: allActs.filter(e => DM[e.deal].co === sc.id).slice(0, 5).map(act), openText: sums(coDeals(sc).filter(isOpen), false, stageOf).map(x => x.text).join(' · ') || '—' };
  const person = { ...sp, company: CM[sp.co].name, owner: av(sp.owner), lastBy: av(sp.by), deals: deals.filter(d => d.cts.includes(sp.id)).map(dv), acts: allActs.filter(e => DM[e.deal].cts.includes(sp.id)).slice(0, 5).map(act) };
  // feed
  const af = { all: () => true, people: e => !AM[e.actor], agents: e => !!AM[e.actor], mine: e => e.actor === ME || e.actor === MY_AGENT }[s.feedActor] || (e => e.actor === s.feedActor);
  const tf = s.feedType === 'all' ? () => true : e => e.type === s.feedType;
  const feedList = allActs.filter(e => af(e) && tf(e)).map(act);
  const feedGroups = ['Today', 'Yesterday', 'Earlier'].map(day => ({ day, items: feedList.filter(e => e.day === day) })).filter(g => g.items.length);
  const feedActors = [['all', 'Everyone'], ['mine', 'Me + Nova'], ['people', 'People'], ['agents', 'Agents']].map(([id, label]) => ({ id, label, active: s.feedActor === id, select: () => set({ feedActor: id }) }));
  const feedPeople = ['u1', 'a1', 'u2', 'a2', 'u3', 'a3', 'a4', 'u5', 'u6', 'u4'].map(id => ({ ...av(id), active: s.feedActor === id, select: () => set({ feedActor: id }), count: allActs.filter(e => e.actor === id).length }));
  const feedTypes = [['all', 'All types'], ['call', 'Calls'], ['email', 'Emails'], ['meeting', 'Meetings'], ['note', 'Notes'], ['stage', 'Stage changes']].map(([id, label]) => ({ id, label, active: s.feedType === id, select: () => set({ feedType: id }) }));
  // home
  const openScoped = scoped.filter(isOpen);
  const pipe = sums(openScoped, false, stageOf), wpipe = sums(openScoped, true, stageOf);
  const maxStage = Math.max(...stages.slice(0, 4).map(st => openScoped.filter(d => stageOf(d) === st.id).length), 1);
  const stageBars = stages.slice(0, 4).map(st => { const l = openScoped.filter(d => stageOf(d) === st.id); return { ...st, color: o.stage[st.id], count: l.length, pct: Math.round(l.length / maxStage * 100) + '%', totalsText: sums(l, false, stageOf).map(x => x.compact).join(' · ') }; });
  const attention = [
    { id: 'x1', kind: 'Approval', a: av('a1'), text: 'Wants to offer 8% discount on depot pricing — above your 5% rule', deal: DM.d1.title, open: openDeal('d1'), action: 'Review', urgent: true },
    { id: 'x2', kind: 'Overdue', a: av(ME), text: 'Chase signed MSA from Meridian', deal: DM.d5.title, open: openDeal('d5'), action: 'Open', urgent: true, due: 'Due yesterday' },
    { id: 'x3', kind: 'Due today', a: av(ME), text: 'Get legal contact at Northwind', deal: DM.d1.title, open: openDeal('d1'), action: 'Open', due: 'Today' },
    { id: 'x4', kind: 'Risk', a: av('a4'), text: 'No reply from Grace Liu on Rotterdam retrofit in 9 days', deal: DM.d15.title, open: openDeal('d15'), action: 'Open' },
    { id: 'x5', kind: 'Mention', a: av('u2'), text: 'Offered an intro to Halvorsen\u2019s fleet lead', deal: DM.d1.title, open: openDeal('d1'), action: 'Reply' },
  ].filter(x => !s.dismissed[x.id]).map(x => ({ ...x, dismiss: () => set({ dismissed: { ...s.dismissed, [x.id]: true } }) }));
  const closingSoon = openScoped.filter(d => closeKey(d.close) <= 1031).sort((a, b) => closeKey(a.close) - closeKey(b.close)).map(dv);
  // reports
  const openAll = deals.filter(isOpen);
  const forecast = CUR.map(cur => {
    const l = openAll.filter(d => d.cur === cur); if (!l.length) return null;
    const part = sts => l.filter(d => sts.includes(stageOf(d))).reduce((a, d) => a + d.value, 0);
    const commit = part(['negotiation']), best = part(['proposal']), early = part(['qualify', 'discovery']), tot = commit + best + early || 1;
    const won = deals.filter(d => d.cur === cur && stageOf(d) === 'won').reduce((a, d) => a + d.value, 0);
    return { cur, commit: money(commit, cur, true), best: money(best, cur, true), early: money(early, cur, true), won: money(won, cur, true), weighted: money(l.reduce((a, d) => a + d.value * SM[stageOf(d)].prob / 100, 0), cur, true),
      cw: Math.round(commit / tot * 100) + '%', bw: Math.round(best / tot * 100) + '%', ew: Math.round(early / tot * 100) + '%', n: l.length };
  }).filter(Boolean);
  const vmax = Math.max(...volume.map(([h, a]) => h + a));
  const vol = volume.map(([h, a], i) => ({ wk: 'W' + (27 + i), h, a, hh: Math.round(h / vmax * 150) + 'px', ah: Math.round(a / vmax * 150) + 'px', label: i % 2 ? '' : 'W' + (27 + i) }));
  const conv = conversion.map(([from, to, pct], i) => ({ from, to, pct: pct + '%', w: pct + '%', color: o.stage[stages[i + 1].id] }));
  // team
  const team = users.map(u => { const l = deals.filter(d => ownerOf(d) === u.id && isOpen(d)); const st = teamStats[u.id]; const ag = agents.find(a => a.owner === u.id); return { ...av(u.id), openCount: l.length, openText: sums(l, false, stageOf).map(x => x.compact).join(' · ') || '—', wr: st.wr ? st.wr + '%' : '—', wrw: st.wr + '%', act: st.act, ag: st.ag, actw: Math.round(st.act / 510 * 100) + '%', agw: Math.round(st.ag / 510 * 100) + '%', won: st.won, agent: ag ? av(ag.id) : null, hasAgent: !!ag }; });
  // settings
  const setTabs = [['users', 'Users & roles'], ['agents', 'Agents'], ['stages', 'Pipeline stages'], ['fields', 'Custom fields']].map(([id, label]) => ({ id, label, active: s.set === id, select: () => set({ set: id }) }));
  // search
  const q = s.q.trim().toLowerCase(), m = x => q && x.toLowerCase().includes(q);
  const sg = [
    { label: 'Deals', items: deals.filter(d => m(d.title) || m(CM[d.co].name)).slice(0, 5).map(d => { const x = dv(d); return { id: d.id, title: d.title, sub: x.company, meta: x.valC + ' · ' + x.stageName, a: x.owner, open: x.open }; }) },
    { label: 'Companies', items: companies.filter(c => m(c.name) || m(c.domain)).slice(0, 4).map(c => ({ id: c.id, title: c.name, sub: c.domain, meta: c.industry, a: av(c.owner), open: () => set({ page: 'people', ptab: 'companies', co: c.id, search: false }) })) },
    { label: 'Contacts', items: contacts.filter(p => m(p.name) || m(CM[p.co].name)).slice(0, 4).map(p => ({ id: p.id, title: p.name, sub: p.title + ' · ' + CM[p.co].name, meta: 'Last ' + p.t, a: av(p.by), open: () => set({ page: 'people', ptab: 'contacts', ct: p.id, search: false }) })) },
    { label: 'Activity', items: allActs.filter(e => m(e.text) || m(CM[DM[e.deal].co].name)).slice(0, 4).map(e => { const x = act(e); return { id: e.id, title: e.text, sub: x.dealTitle, meta: e.t, a: x.a, open: x.open }; }) },
  ].filter(g => g.items.length);
  const navDefs = [['home', 'Home', ''], ['activity', 'Inbox', '12'], ['board', 'Pipeline', ''], ['deals', 'Deals', deals.filter(isOpen).length], ['people', 'Contacts & companies', ''], ['reports', 'Reports', ''], ['team', 'Team', ''], ['settings', 'Settings', '']];
  const pageOf = s.page === 'record' ? 'deals' : s.page;
  return {
    mode: s.mode, dark: s.mode === 'dark', toggleMode: () => set({ mode: s.mode === 'dark' ? 'light' : 'dark' }), modeLabel: s.mode === 'dark' ? 'Light' : 'Dark',
    is: { home: s.page === 'home', board: s.page === 'board', deals: s.page === 'deals', record: s.page === 'record', people: s.page === 'people', activity: s.page === 'activity', reports: s.page === 'reports', team: s.page === 'team', settings: s.page === 'settings' },
    nav: navDefs.map(([id, label, count]) => ({ id, label, count, hasCount: count !== '', active: pageOf === id, go: go(id) })),
    go: { home: go('home'), board: go('board'), deals: go('deals'), people: go('people'), activity: go('activity'), reports: go('reports'), team: go('team'), settings: go('settings') },
    pageTitle: { home: 'Home', board: 'Pipeline', deals: 'Deals', record: 'Deal', people: 'Contacts & companies', activity: 'Inbox', reports: 'Reports', team: 'Team', settings: 'Settings' }[s.page],
    me: av(ME), myAgent: av(MY_AGENT), online: users.filter(u => u.online).map(u => av(u.id)), agentsList: agents.map(a => av(a.id)), onlineCount: 50 - 29,
    scope: { team: s.scope === 'team', mine: s.scope === 'mine', setTeam: () => set({ scope: 'team' }), setMine: () => set({ scope: 'mine' }) },
    columns, openColumns: columns.slice(0, 4), closedColumns: columns.slice(4), toast, hasToast: !!toast, replay: () => replay(self), agentWorking: s.phase === 0 && s.ov.d1 === undefined,
    views: views.map(v => ({ ...v, personal: v.who === 'me', active: v.id === s.view, select: () => set({ view: v.id, filterOpen: false }), count: deals.filter(d => (s.scope === 'team' || mineF(d)) && ({ open: isOpen, mine: x => isOpen(x) && mineF(x), closing: x => isOpen(x) && closeKey(x.close) <= 1031, eur: x => isOpen(x) && x.cur === 'EUR', agents: x => AM[x.by] && /m|h/.test(x.t), stalled: () => false, closed: x => !isOpen(x) })[v.id](d)).length })),
    view: { ...vw, personal: vw.who === 'me', filters: vw.filters.map((f, i) => { const [field, ...rest] = f.split(' is '); return { text: f, field: rest.length ? field : f, op: rest.length ? 'is' : '', value: rest.join(' is ') }; }) },
    filterOpen: s.filterOpen, toggleFilter: () => set({ filterOpen: !s.filterOpen }), closeFilter: () => set({ filterOpen: false }),
    rows: rowsRaw.map(dv), rowCount: rowsRaw.length, rowsEmpty: rowsRaw.length === 0, rowsTotals: sums(rowsRaw, false, stageOf), rowsTotalsText: sums(rowsRaw, false, stageOf).map(x => x.text).join('  ·  '),
    resetView: () => set({ view: 'open' }), sortOn, arrow,
    rec, recCompany: { ...rc, owner: av(rc.owner) }, recContacts: rd.cts.map((p, i) => ({ ...PM[p], role: ['Champion', 'Economic buyer', 'Technical evaluator'][i] || 'Contact', lastBy: av(PM[p].by) })),
    recActs, recTasks, recOpenTasks: recTasks.filter(t => !t.done), recDoneTasks: recTasks.filter(t => t.done), recNotes, viewers, related, stagePath,
    recTabs: tabDefs.map(([id, label, count]) => ({ id, label, count, hasCount: count !== '' && count !== 0, active: s.tab === id, select: () => set({ tab: id }) })),
    tab: { overview: s.tab === 'overview', activity: s.tab === 'activity', notes: s.tab === 'notes', tasks: s.tab === 'tasks', related: s.tab === 'related' },
    recWeighted: money(rd.value * rec.prob / 100, rd.cur), recNext: recTasks.find(t => !t.done),
    assignOpen: s.assignOpen, toggleAssign: () => set({ assignOpen: !s.assignOpen }),
    assignees: users.map(u => ({ ...av(u.id), current: ownerOf(rd) === u.id, pick: () => set({ own: { ...s.own, [rd.id]: u.id }, assignOpen: false, userActs: [{ id: 'u' + Date.now(), actor: ME, type: 'field', deal: rd.id, text: `Reassigned owner → ${u.name}`, t: 'just now', day: 'Today' }, ...s.userActs] }) })),
    ptab: { companies: s.ptab === 'companies', contacts: s.ptab === 'contacts', toCompanies: () => set({ ptab: 'companies' }), toContacts: () => set({ ptab: 'contacts' }) },
    coRows, ctRows, company, person, nCompanies: companies.length, nContacts: contacts.length,
    feedGroups, feedActors, feedPeople, feedTypes, feedCount: feedList.length, feedEmpty: feedList.length === 0,
    pipe, wpipe, stageBars, attention, closingSoon, nOpen: openScoped.length,
    agentFeed: allActs.filter(e => AM[e.actor]).slice(0, 7).map(act), recentFeed: allActs.slice(0, 8).map(act),
    myTasks: tasks.filter(t => !t.done && (t.who === ME || t.who === MY_AGENT)).map(t => ({ ...t, a: av(t.who), dealTitle: DM[t.deal].title, open: openDeal(t.deal) })),
    forecast, vol, conv, team, fields, setTabs, setIs: { users: s.set === 'users', agents: s.set === 'agents', stages: s.set === 'stages', fields: s.set === 'fields' },
    users: users.map(u => ({ ...av(u.id), agent2: agents.find(a => a.owner === u.id) ? av(agents.find(a => a.owner === u.id).id) : null })),
    stageSettings: stages.map(st => ({ ...st, color: o.stage[st.id], probText: st.prob + '%', count: deals.filter(d => stageOf(d) === st.id).length })),
    search: s.search, q: s.q, onQ: e => set({ q: e.target.value }), openSearch: () => set({ search: true }), closeSearch: () => set({ search: false }), stop: e => e.stopPropagation(),
    searchGroups: sg, searchEmpty: sg.length === 0,
  };
}
