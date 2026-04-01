# Commerce workflow playbook — online offer, from supplier to customer

Use this as the **default pipeline** when a business sells physical or digital goods online. Adapt steps to the vertical; the **roles** map to pack personas.

## 1. Example: design & vintage furniture

1. **Sourcing (`supplier_sourcing`)**  
   Clarify per piece or line: provenance, reproduction vs original, materials, lead time, MOQ, customization bounds, packaging, damage rate history, compliance (flammability, wood species restrictions if any).

2. **Creative (`product_creative`)**  
   Shot list, room sets, storytelling (craft, era, designer influence). Align with what suppliers can actually deliver and photograph.

3. **Merchandising (`merchandising_copy`)**  
   PDPs, collections, SEO, care instructions. Dimensions and materials must trace to supplier confirmation.

4. **Social (`social_media_manager`)**  
   Pillars: behind-the-scenes, room inspiration, drops, education (how to judge quality). Reuse hooks from PDP themes.

5. **Logistics (`logistics_fulfillment`)**  
   Freight vs parcel, white-glove, international duties disclaimer, returns for bulky goods, damage claims playbook.

6. **Orchestration (`growth_orchestrator`)**  
   Sequencing, margin checks, stock narrative vs marketing promises, escalation when any step blocks the next.

## 2. Generic pattern (any online offer)

| Stage | Question answered |
|--------|-------------------|
| **Offer definition** | What exactly is sold, in what unit, with what variants? |
| **Supply / delivery** | Who produces or ships, on what SLA? |
| **Story & proof** | Why trust us—reviews, data, process? |
| **Acquisition** | Organic, paid, marketplace, partnerships? |
| **Conversion** | PDP, checkout, objections, guarantees (only if real). |
| **Post-purchase** | Tracking, returns, support, LTV / repeat. |

## 3. Handoffs

Use **structured briefs** between personas: bullet specs, open questions, and explicit **owner**. If you use HSM-II enterprise ops, mirror work as **tickets** in `config/operations.yaml` so `list_tickets` / `read_operations` stay authoritative.

## 4. What this pack does *not* do

- It does not replace ERP, WMS, or carrier APIs.  
- It does not auto-place supplier orders or charge cards.  
- It helps **draft, plan, and optimize communication** under human approval.
