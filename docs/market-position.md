# Market position — an honest read

**Date:** 2026-09-08 · **Author:** PM · **Status:** for the product owner, not for customers

## The verdict, up front

**No, firms would not pick Breksos CRM as their CRM today** — and not because it is unfinished.
Because a CRM is a *team* system of record, and ours is single-user by construction. That is a
category mismatch, not a feature gap.

**Do we have an upper hand? One, and it is real** — but it is a wedge, not an advantage. We
have a genuinely novel interaction model and roughly six table-stakes gaps. Those are not the
same size of thing.

The useful reframe: **this is not a CRM company. It is a demonstration of an interaction
model, using a domain everyone already understands.** Judged as a CRM it loses. Judged as
proof that agent-native software can be watched rather than trusted blindly, it is early and
interesting.

## What we are walking into

The market is mature, cheap and crowded. Pipedrive is $14/seat, Attio and HubSpot around
$20–29, folk $25, and HubSpot has a genuinely usable free tier
([automaiva](https://automaiva.com/folk-vs-hubspot-vs-pipedrive-vs-attio-crm/),
[authencio](https://www.authencio.com/blog/attio-crm-review-features-pricing-customization-alternatives)).
A firm evaluating CRMs has a dozen good, supported, integrated options before it hears of us.

### The finding that hurts most

**Agent-native CRM is not an empty field. It is already a funded, shipping category.**

- Salesforce **Agentforce**: ~$540M ARR, 18,500 customers, 3B workflows/month
- HubSpot **Breeze**: 279K+ customers
- Gartner expects 40% of enterprise applications to embed AI agents by end of 2026
- **HubSpot already ships a production-grade MCP server**; Salesforce has a native MCP client
  in pilot
  ([digitalapplied](https://www.digitalapplied.com/blog/crm-ai-agent-salesforce-hubspot-zoho-2026-guide),
  [vantagepoint](https://vantagepoint.io/blog/sf/hubspot-vs-salesforce-ai-agent-ready-2026-comparison))

That last point is the sharpest one. **An agent can already read and write a real CRM without
our app existing.** If the pitch is "your agent can use your CRM," HubSpot answered it, for
free, inside the tool the customer already has. We must not pitch that.

### But their agents and ours are not the same thing

The incumbents built **agent-as-backend-automation**: agents run in the vendor's cloud,
execute workflows, and you inspect the results afterwards. Nobody is sitting there watching.

Ours is **agent-as-visible-collaborator**: one surface, two operators, and every action the
agent takes appears on the screen the person is already looking at — attributed to it by name.

That difference is not cosmetic, and the market is only now discovering it matters. Honeycomb
shipped agent observability in 2026; there is active research on interfaces for *observing and
steering* long-running agents because "a messy transcript of reasoning steps and tool calls"
makes supervision hard; and at least one startup is building a shared human/agent workspace
([honeycomb](https://www.honeycomb.io/blog/honeycomb-launches-agent-observability-bringing-full-visibility-to-agentic-workflows),
[AgentGUI, arXiv](https://arxiv.org/html/2607.26300v2)).

**But all of that is aimed at engineers supervising coding agents.** Nobody has brought it to
business software, where the person watching is not technical and the record being written is
a customer relationship. That gap is ours if we want it.

## Where we lose, plainly

| Gap | Severity |
|---|---|
| **Single user, no shared data.** A CRM is a team tool by definition. | **Fatal for firm adoption** |
| No email or calendar sync — the number-one source of CRM data | Severe |
| No mobile, no web access | Severe |
| No integrations, no marketplace | Severe |
| **Distribution is Clatch**, a launcher almost nobody has installed | **Structural** |
| Reminders only fire while the app is open (no scheduler, by platform design) | Moderate, and it is a trust problem more than a feature one |
| Unknown vendor: no SOC 2, no DPA, no support contract, no procurement story | Blocking above ~20 seats |

The distribution one deserves emphasis because it is not a roadmap item. A firm cannot adopt
Breksos CRM without first adopting Clatch. We are not selling a CRM into the CRM market; we
are asking for a platform decision and a CRM decision at once.

## Where we genuinely win

1. **Co-presence.** The human and the agent look at the same live board. No CRM has this.
   Incumbent agents are invisible until they finish.
2. **Per-record attribution.** Every activity records whether a person or a *named* agent
   wrote it. As agents start writing into systems of record, "who put this here" stops being a
   nicety. This is the feature I would lead with, and it is cheap for us because the platform
   hands us agent identity.
3. **A real CLI, not an API.** The agent gets a first-class typed surface with no rate limit,
   no quota, no OAuth dance — and permissions granular per verb, so a firm can grant read-only.
4. **Local, zero-latency, no per-seat cost.** There is real and growing appetite for
   self-hosted and data-sovereign CRM ([growcrm](https://growcrm.io/2026/01/04/top-20-open-source-self-hosted-crms-in-2025/),
   [jm-origin](https://www.jm-origin.com/blogs/best-privacy-first-crm-solutions-2026/)) — but note
   that niche skews *personal*, not *firm*, which is the same trap as above.

## Who would actually adopt this

Not "firms". **Individual operators who already live with an agent** — a solo founder, an
independent consultant, a two-person agency, a technical salesperson. The Superhuman/Raycast
adoption pattern: one person adopts it personally, on their own machine, without procurement.

That is a real market and a bad venture market. It is an excellent *proof* market.

## What would change the answer

Ranked strictly by leverage:

1. **Multi-seat with shared data.** Without it, "firm adoption" is definitionally impossible.
   Everything else is decoration until this exists — and it is a deep change, because our
   entire architecture assumes one machine and one person.
2. **Email and calendar ingestion.** The single largest source of CRM content, and today we
   have deliberately decided not to reach outside the machine.
3. **Distribution outside Clatch**, or Clatch itself reaching meaningful installs.
4. **Mobile read access.** A salesperson checks a deal in a car park.
5. **A hosted option** for teams unwilling to run local software.

Items 1–3 are each larger than everything on the current roadmap combined. That is the honest
scale.

## My recommendation

**Do not reposition this as a CRM product, and do not add features chasing firm adoption.**
That race is lost on cost, integrations and distribution before it starts.

Instead:

- **Finish v1 as specified.** It is a strong proof of the clapp model on a domain nobody has
  to have explained to them.
- **Lead with attribution and co-presence**, never with "your agent can use your CRM" — the
  incumbents answered that one already and for free.
- **Treat the single-operator user as the real user**, and make the app excellent for them
  rather than adequate for a team it cannot serve.
- **Decide separately, and deliberately, whether the goal is a product or a demonstration.**
  Those need different roadmaps, and doing both badly is the common failure. My read is that
  the demonstration is the valuable asset here, and that the interaction model — not the CRM —
  is the thing worth owning.

The timing argument is genuinely in our favour: agent observability is becoming a category
right now, and nobody has brought it to business software. Being early matters. Being early
with no distribution and no multi-user is also exactly how good ideas die, and both things are
true at once.

---

**Sources:**
[automaiva — CRM pricing comparison](https://automaiva.com/folk-vs-hubspot-vs-pipedrive-vs-attio-crm/) ·
[authencio — Attio review](https://www.authencio.com/blog/attio-crm-review-features-pricing-customization-alternatives) ·
[digitalapplied — CRM AI agents 2026](https://www.digitalapplied.com/blog/crm-ai-agent-salesforce-hubspot-zoho-2026-guide) ·
[vantagepoint — HubSpot vs Salesforce agent-readiness](https://vantagepoint.io/blog/sf/hubspot-vs-salesforce-ai-agent-ready-2026-comparison) ·
[Honeycomb — agent observability](https://www.honeycomb.io/blog/honeycomb-launches-agent-observability-bringing-full-visibility-to-agentic-workflows) ·
[AgentGUI (arXiv)](https://arxiv.org/html/2607.26300v2) ·
[growcrm — self-hosted CRMs](https://growcrm.io/2026/01/04/top-20-open-source-self-hosted-crms-in-2025/) ·
[jm-origin — privacy-first CRM](https://www.jm-origin.com/blogs/best-privacy-first-crm-solutions-2026/)
