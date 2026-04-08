use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HsmNativeTurn {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HsmNativeSession {
    pub session_id: u32,
    pub agent: String,
    pub turns: Vec<HsmNativeTurn>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HsmNativeGold {
    pub answer: String,
    #[serde(default)]
    pub required_facts: Vec<String>,
    #[serde(default)]
    pub forbidden_stale_facts: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HsmNativeTask {
    pub id: String,
    pub suite: String,
    pub sessions: Vec<HsmNativeSession>,
    pub question: String,
    pub gold: HsmNativeGold,
}

pub fn built_in_hsm_native_tasks() -> Vec<HsmNativeTask> {
    vec![
        HsmNativeTask {
            id: "cross-session-001".into(),
            suite: "cross_session_synthesis".into(),
            sessions: vec![
                HsmNativeSession {
                    session_id: 1,
                    agent: "planner".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "For the analytics product, keep the backend in Rust because low latency matters more than rapid prototyping.".into(),
                        },
                        HsmNativeTurn {
                            role: "assistant".into(),
                            content: "Noted: Rust backend for the analytics product due to latency requirements.".into(),
                        },
                    ],
                },
                HsmNativeSession {
                    session_id: 2,
                    agent: "ops".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "Store customer uploads in S3 and keep Postgres only for metadata.".into(),
                        },
                        HsmNativeTurn {
                            role: "assistant".into(),
                            content: "Understood: S3 for uploads, Postgres for metadata.".into(),
                        },
                    ],
                },
            ],
            question: "What architecture should we use for the analytics product?".into(),
            gold: HsmNativeGold {
                answer: "Use a Rust backend with S3 for customer uploads and Postgres for metadata."
                    .into(),
                required_facts: vec!["rust".into(), "s3".into(), "postgres".into()],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "cross-session-002".into(),
            suite: "cross_session_synthesis".into(),
            sessions: vec![
                HsmNativeSession {
                    session_id: 1,
                    agent: "finance".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "Keep the pilot budget under $40k.".into(),
                        },
                        HsmNativeTurn {
                            role: "assistant".into(),
                            content: "Pilot budget cap recorded: $40k.".into(),
                        },
                    ],
                },
                HsmNativeSession {
                    session_id: 2,
                    agent: "sales".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "Run the pilot only with Canadian customers because procurement is already cleared there.".into(),
                        },
                        HsmNativeTurn {
                            role: "assistant".into(),
                            content: "Understood: Canada-only pilot.".into(),
                        },
                    ],
                },
            ],
            question: "What constraints should the pilot plan follow?".into(),
            gold: HsmNativeGold {
                answer: "Keep the pilot under $40k and restrict it to Canadian customers.".into(),
                required_facts: vec!["40k".into(), "canadian".into()],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "cross-session-003".into(),
            suite: "cross_session_synthesis".into(),
            sessions: vec![
                HsmNativeSession {
                    session_id: 1,
                    agent: "product".into(),
                    turns: vec![HsmNativeTurn {
                        role: "user".into(),
                        content: "The mobile rollout must start with field technicians because they work offline most of the day.".into(),
                    }],
                },
                HsmNativeSession {
                    session_id: 2,
                    agent: "engineering".into(),
                    turns: vec![HsmNativeTurn {
                        role: "user".into(),
                        content: "To support field technicians, the app needs an offline-first local queue with background sync once connectivity returns.".into(),
                    }],
                },
                HsmNativeSession {
                    session_id: 3,
                    agent: "security".into(),
                    turns: vec![HsmNativeTurn {
                        role: "user".into(),
                        content: "Offline device data must be encrypted at rest with AES-256 because these are unmanaged tablets.".into(),
                    }],
                },
            ],
            question: "What must the first mobile rollout include?".into(),
            gold: HsmNativeGold {
                answer: "It should target field technicians and include an offline-first local queue with background sync plus AES-256 encryption at rest.".into(),
                required_facts: vec![
                    "field technicians".into(),
                    "offline".into(),
                    "background sync".into(),
                    "aes 256".into(),
                ],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "cross-session-004".into(),
            suite: "cross_session_synthesis".into(),
            sessions: vec![
                HsmNativeSession {
                    session_id: 1,
                    agent: "sales".into(),
                    turns: vec![HsmNativeTurn {
                        role: "user".into(),
                        content: "The first lighthouse account is Northwind Bank, and they only buy if SSO is ready.".into(),
                    }],
                },
                HsmNativeSession {
                    session_id: 2,
                    agent: "finance".into(),
                    turns: vec![HsmNativeTurn {
                        role: "user".into(),
                        content: "Do not discount below $48k ARR in the first enterprise deal.".into(),
                    }],
                },
                HsmNativeSession {
                    session_id: 3,
                    agent: "implementation".into(),
                    turns: vec![HsmNativeTurn {
                        role: "user".into(),
                        content: "Northwind's procurement window closes on September 15, so SSO must be ready before then.".into(),
                    }],
                },
            ],
            question: "What must we do to close the first lighthouse account?".into(),
            gold: HsmNativeGold {
                answer: "Have SSO ready for Northwind Bank before September 15 and keep the first enterprise deal at or above $48k ARR.".into(),
                required_facts: vec![
                    "northwind".into(),
                    "sso".into(),
                    "september 15".into(),
                    "48k".into(),
                ],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "belief-revision-001".into(),
            suite: "belief_revision".into(),
            sessions: vec![
                HsmNativeSession {
                    session_id: 1,
                    agent: "ops".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "The supplier said the replacement batch will arrive on Monday.".into(),
                        },
                    ],
                },
                HsmNativeSession {
                    session_id: 2,
                    agent: "ops".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "Update: the supplier moved the replacement batch to Wednesday. Monday is no longer correct.".into(),
                        },
                    ],
                },
            ],
            question: "When does the replacement batch arrive now?".into(),
            gold: HsmNativeGold {
                answer: "It arrives on Wednesday.".into(),
                required_facts: vec!["wednesday".into()],
                forbidden_stale_facts: vec!["monday".into()],
            },
        },
        HsmNativeTask {
            id: "belief-revision-002".into(),
            suite: "belief_revision".into(),
            sessions: vec![
                HsmNativeSession {
                    session_id: 1,
                    agent: "api".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "Tell clients to integrate against API v1 for now.".into(),
                        },
                    ],
                },
                HsmNativeSession {
                    session_id: 2,
                    agent: "api".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "Revision: API v2 is now stable and all new clients should use v2 instead of v1.".into(),
                        },
                    ],
                },
            ],
            question: "Which API version should new clients use?".into(),
            gold: HsmNativeGold {
                answer: "New clients should use API v2.".into(),
                required_facts: vec!["v2".into()],
                forbidden_stale_facts: vec!["v1".into()],
            },
        },
        HsmNativeTask {
            id: "handoff-001".into(),
            suite: "agent_handoff".into(),
            sessions: vec![
                HsmNativeSession {
                    session_id: 1,
                    agent: "researcher".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "The best early adopter is St. Mary's Hospital because they already approved a June 12 pilot review.".into(),
                        },
                    ],
                },
                HsmNativeSession {
                    session_id: 2,
                    agent: "operator".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "The finisher should send the pilot checklist before the June 12 review with St. Mary's Hospital.".into(),
                        },
                    ],
                },
            ],
            question: "What should the finisher do next?".into(),
            gold: HsmNativeGold {
                answer: "Send the pilot checklist to St. Mary's Hospital before the June 12 review."
                    .into(),
                required_facts: vec!["st. mary".into(), "pilot checklist".into(), "june 12".into()],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "handoff-002".into(),
            suite: "agent_handoff".into(),
            sessions: vec![
                HsmNativeSession {
                    session_id: 1,
                    agent: "designer".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "We chose a navy header with the condensed title font 'Oswald' for the launch page.".into(),
                        },
                    ],
                },
                HsmNativeSession {
                    session_id: 2,
                    agent: "pm".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "Tell engineering to preserve the navy header and Oswald title font in the first implementation.".into(),
                        },
                    ],
                },
            ],
            question: "What design details must engineering preserve?".into(),
            gold: HsmNativeGold {
                answer: "Engineering should preserve the navy header and the Oswald title font."
                    .into(),
                required_facts: vec!["navy".into(), "oswald".into()],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "handoff-003".into(),
            suite: "agent_handoff".into(),
            sessions: vec![
                HsmNativeSession {
                    session_id: 1,
                    agent: "researcher".into(),
                    turns: vec![HsmNativeTurn {
                        role: "user".into(),
                        content: "The win theme in customer interviews was faster month-end close for controllers in multi-entity finance teams.".into(),
                    }],
                },
                HsmNativeSession {
                    session_id: 2,
                    agent: "pm".into(),
                    turns: vec![HsmNativeTurn {
                        role: "user".into(),
                        content: "The launch copy draft should emphasize faster month-end close, specifically for multi-entity finance controllers.".into(),
                    }],
                },
                HsmNativeSession {
                    session_id: 3,
                    agent: "editor".into(),
                    turns: vec![HsmNativeTurn {
                        role: "user".into(),
                        content: "The finisher needs to ship the revised homepage headline before tomorrow's review.".into(),
                    }],
                },
            ],
            question: "What should the homepage headline emphasize before tomorrow's review?".into(),
            gold: HsmNativeGold {
                answer: "The homepage headline should emphasize faster month-end close for multi-entity finance controllers before tomorrow's review.".into(),
                required_facts: vec![
                    "month-end close".into(),
                    "multi-entity".into(),
                    "controllers".into(),
                ],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "policy-001".into(),
            suite: "policy_persistence".into(),
            sessions: vec![
                HsmNativeSession {
                    session_id: 1,
                    agent: "founder".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "Company policy: reject fixed-fee enterprise work under $25k because it destroys margin.".into(),
                        },
                    ],
                },
                HsmNativeSession {
                    session_id: 2,
                    agent: "sales".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "A prospect wants a fixed-fee enterprise engagement for $18k.".into(),
                        },
                    ],
                },
            ],
            question: "Should we accept the $18k fixed-fee enterprise deal?".into(),
            gold: HsmNativeGold {
                answer: "No, reject it because company policy forbids fixed-fee enterprise work under $25k."
                    .into(),
                required_facts: vec!["no".into(), "25k".into(), "fixed-fee".into()],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "policy-002".into(),
            suite: "policy_persistence".into(),
            sessions: vec![
                HsmNativeSession {
                    session_id: 1,
                    agent: "security".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "Policy: do not ingest protected healthcare data before SOC 2 is complete.".into(),
                        },
                    ],
                },
                HsmNativeSession {
                    session_id: 2,
                    agent: "sales".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "A clinic wants us to ingest protected healthcare data this week, but SOC 2 is still in progress.".into(),
                        },
                    ],
                },
            ],
            question: "Can we ingest the clinic's protected healthcare data this week?".into(),
            gold: HsmNativeGold {
                answer: "No, not until SOC 2 is complete.".into(),
                required_facts: vec!["no".into(), "soc 2".into()],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "conflict-001".into(),
            suite: "conflict_resolution".into(),
            sessions: vec![
                HsmNativeSession {
                    session_id: 1,
                    agent: "infra".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "We initially planned to run the EU workload from Frankfurt.".into(),
                        },
                    ],
                },
                HsmNativeSession {
                    session_id: 2,
                    agent: "infra".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "Final decision: move the EU workload to Dublin because the data processor agreement is already signed there.".into(),
                        },
                    ],
                },
            ],
            question: "Which city should host the EU workload?".into(),
            gold: HsmNativeGold {
                answer: "Dublin should host the EU workload.".into(),
                required_facts: vec!["dublin".into()],
                forbidden_stale_facts: vec!["frankfurt".into()],
            },
        },
        HsmNativeTask {
            id: "conflict-002".into(),
            suite: "conflict_resolution".into(),
            sessions: vec![
                HsmNativeSession {
                    session_id: 1,
                    agent: "ops".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "The customer review was first scheduled for Tuesday.".into(),
                        },
                    ],
                },
                HsmNativeSession {
                    session_id: 2,
                    agent: "ops".into(),
                    turns: vec![
                        HsmNativeTurn {
                            role: "user".into(),
                            content: "Correction: the customer review moved to Thursday after the executive conflict.".into(),
                        },
                    ],
                },
            ],
            question: "When is the customer review meeting now?".into(),
            gold: HsmNativeGold {
                answer: "The meeting is now on Thursday.".into(),
                required_facts: vec!["thursday".into()],
                forbidden_stale_facts: vec!["tuesday".into()],
            },
        },
        HsmNativeTask {
            id: "cross-session-005".into(),
            suite: "cross_session_synthesis".into(),
            sessions: vec![
                HsmNativeSession { session_id: 1, agent: "research".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "The compliance dashboard should target hospital CFOs first.".into() }] },
                HsmNativeSession { session_id: 2, agent: "design".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "Hospital CFOs asked for a weekly risk digest email, not a dense real-time console.".into() }] },
                HsmNativeSession { session_id: 3, agent: "engineering".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "The weekly risk digest must pull from the existing warehouse instead of a new streaming pipeline.".into() }] },
            ],
            question: "What should the first compliance dashboard release include?".into(),
            gold: HsmNativeGold {
                answer: "It should target hospital CFOs and include a weekly risk digest email powered from the existing warehouse.".into(),
                required_facts: vec!["hospital cfo".into(), "weekly risk digest".into(), "warehouse".into()],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "cross-session-006".into(),
            suite: "cross_session_synthesis".into(),
            sessions: vec![
                HsmNativeSession { session_id: 1, agent: "sales".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "The telecom pilot only works if call summaries are available in Spanish.".into() }] },
                HsmNativeSession { session_id: 2, agent: "product".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "Spanish summaries must be editable in the supervisor review screen before export.".into() }] },
                HsmNativeSession { session_id: 3, agent: "finance".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "Keep the telecom pilot to one region so support cost stays under budget.".into() }] },
            ],
            question: "What are the requirements for the telecom pilot?".into(),
            gold: HsmNativeGold {
                answer: "Provide Spanish call summaries, allow supervisor editing before export, and keep the pilot to one region.".into(),
                required_facts: vec!["spanish".into(), "editing".into(), "one region".into()],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "cross-session-007".into(),
            suite: "cross_session_synthesis".into(),
            sessions: vec![
                HsmNativeSession { session_id: 1, agent: "ops".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "The field audit app has to work on old Android tablets.".into() }] },
                HsmNativeSession { session_id: 2, agent: "design".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "Old Android tablet users need oversized checklist controls for glove use.".into() }] },
                HsmNativeSession { session_id: 3, agent: "platform".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "Because connectivity is poor, checklist completion must sync in batches after reconnection.".into() }] },
            ],
            question: "What should the field audit app optimize for?".into(),
            gold: HsmNativeGold {
                answer: "Optimize for old Android tablets with oversized checklist controls and batch sync after reconnection.".into(),
                required_facts: vec!["old android".into(), "oversized".into(), "batch sync".into()],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "belief-revision-003".into(),
            suite: "belief_revision".into(),
            sessions: vec![
                HsmNativeSession { session_id: 1, agent: "success".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "The onboarding call is booked for April 8.".into() }] },
                HsmNativeSession { session_id: 2, agent: "success".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "Correction: the onboarding call moved to April 11, so April 8 is obsolete.".into() }] },
            ],
            question: "When is the onboarding call now?".into(),
            gold: HsmNativeGold {
                answer: "The onboarding call is on April 11.".into(),
                required_facts: vec!["april 11".into()],
                forbidden_stale_facts: vec!["april 8".into()],
            },
        },
        HsmNativeTask {
            id: "belief-revision-004".into(),
            suite: "belief_revision".into(),
            sessions: vec![
                HsmNativeSession { session_id: 1, agent: "pricing".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "The annual plan is $9,000 for the beta launch.".into() }] },
                HsmNativeSession { session_id: 2, agent: "pricing".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "Update: the annual plan is now $10,500 after finance review; stop quoting $9,000.".into() }] },
            ],
            question: "What annual price should we quote now?".into(),
            gold: HsmNativeGold {
                answer: "Quote $10,500 annually.".into(),
                required_facts: vec!["10 500".into()],
                forbidden_stale_facts: vec!["9 000".into()],
            },
        },
        HsmNativeTask {
            id: "belief-revision-005".into(),
            suite: "belief_revision".into(),
            sessions: vec![
                HsmNativeSession { session_id: 1, agent: "infra".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "Keep the backup window on Saturday night.".into() }] },
                HsmNativeSession { session_id: 2, agent: "infra".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "Revision: move the backup window to Sunday night because Saturday overlaps billing.".into() }] },
            ],
            question: "When is the backup window now?".into(),
            gold: HsmNativeGold {
                answer: "The backup window is now Sunday night.".into(),
                required_facts: vec!["sunday".into()],
                forbidden_stale_facts: vec!["saturday".into()],
            },
        },
        HsmNativeTask {
            id: "handoff-004".into(),
            suite: "agent_handoff".into(),
            sessions: vec![
                HsmNativeSession { session_id: 1, agent: "researcher".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "The procurement blocker was legal review of the vendor security addendum.".into() }] },
                HsmNativeSession { session_id: 2, agent: "pm".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "The finisher should send the redlined vendor security addendum back to legal today.".into() }] },
            ],
            question: "What must the finisher send today?".into(),
            gold: HsmNativeGold {
                answer: "Send the redlined vendor security addendum back to legal today.".into(),
                required_facts: vec!["redlined".into(), "security addendum".into(), "legal".into()],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "handoff-005".into(),
            suite: "agent_handoff".into(),
            sessions: vec![
                HsmNativeSession { session_id: 1, agent: "analyst".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "The retention deck won because it highlighted fewer manual reconciliations for finance ops.".into() }] },
                HsmNativeSession { session_id: 2, agent: "writer".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "The finisher should update the keynote opener to stress fewer manual reconciliations for finance ops.".into() }] },
            ],
            question: "What should the keynote opener stress?".into(),
            gold: HsmNativeGold {
                answer: "It should stress fewer manual reconciliations for finance ops.".into(),
                required_facts: vec!["fewer manual reconciliations".into(), "finance ops".into()],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "policy-003".into(),
            suite: "policy_persistence".into(),
            sessions: vec![
                HsmNativeSession { session_id: 1, agent: "legal".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "Policy: do not sign custom DPAs for pilots under $30k ARR.".into() }] },
                HsmNativeSession { session_id: 2, agent: "sales".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "A prospect wants a $22k ARR pilot and asked for a custom DPA.".into() }] },
            ],
            question: "Should we sign the custom DPA for this pilot?".into(),
            gold: HsmNativeGold {
                answer: "No, do not sign a custom DPA for a $22k ARR pilot.".into(),
                required_facts: vec!["no".into(), "custom dpa".into(), "30k".into()],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "policy-004".into(),
            suite: "policy_persistence".into(),
            sessions: vec![
                HsmNativeSession { session_id: 1, agent: "security".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "Policy: production database access requires a ticket plus two-person approval.".into() }] },
                HsmNativeSession { session_id: 2, agent: "ops".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "An engineer wants direct production database access tonight with only a Slack request.".into() }] },
            ],
            question: "Can the engineer get production database access tonight?".into(),
            gold: HsmNativeGold {
                answer: "No, production database access requires a ticket and two-person approval.".into(),
                required_facts: vec!["no".into(), "ticket".into(), "two-person approval".into()],
                forbidden_stale_facts: vec![],
            },
        },
        HsmNativeTask {
            id: "conflict-003".into(),
            suite: "conflict_resolution".into(),
            sessions: vec![
                HsmNativeSession { session_id: 1, agent: "product".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "We first planned to launch the beta in May.".into() }] },
                HsmNativeSession { session_id: 2, agent: "product".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "Final update: beta launch is June after the customer advisory board requested more reporting.".into() }] },
            ],
            question: "When is the beta launch?".into(),
            gold: HsmNativeGold {
                answer: "The beta launch is in June.".into(),
                required_facts: vec!["june".into()],
                forbidden_stale_facts: vec!["may".into()],
            },
        },
        HsmNativeTask {
            id: "conflict-004".into(),
            suite: "conflict_resolution".into(),
            sessions: vec![
                HsmNativeSession { session_id: 1, agent: "ops".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "The warehouse migration was originally assigned to Team Blue.".into() }] },
                HsmNativeSession { session_id: 2, agent: "ops".into(), turns: vec![HsmNativeTurn { role: "user".into(), content: "Correction: the warehouse migration is now owned by Team Green because Blue is on incident duty.".into() }] },
            ],
            question: "Which team owns the warehouse migration now?".into(),
            gold: HsmNativeGold {
                answer: "Team Green owns the warehouse migration now.".into(),
                required_facts: vec!["team green".into()],
                forbidden_stale_facts: vec!["team blue".into()],
            },
        },
    ]
}
