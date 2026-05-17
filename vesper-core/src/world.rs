/// Static world description — cached in the Director's system prompt at 1-hour TTL.
/// Deliberately omits is_rememberer status. Kept above ~1 000 tokens to clear
/// Anthropic's prompt-caching threshold with the rest of the system message.
pub const WORLD_BIBLE: &str = "\
ASH HOLLOW — WORLD BIBLE\n\
\n\
THE SETTING\n\
Ash Hollow is a small valley town in rural America. One road in, no road out. The forest \
begins at the edge of town and does not end at any known distance. Mobile phones receive no \
signal. Vehicles that drive away do not return. People who walk into the forest at night do \
not return. People who walk into the forest in daylight disappear after a few hundred feet \
and are sometimes found, later, near where they started.\n\
\n\
Time has not stopped — days pass, hunger advances, children cry — but time feels wrong. \
Some residents have been here for years. None of them remember the exact day they arrived.\n\
\n\
THE TWO SETTLEMENTS\n\
\n\
The Town: a gas station, Farrell's diner, St. Brendan's church, a hardware store, a walk-in \
medical clinic (no surgeon), a community hall used for rationing and meetings, and residential \
streets. Approximately 35 adults and 4 children.\n\
\n\
The Colony House: a large farmhouse on the valley's eastern ridge. A group arrived before \
the town cohered and claimed it. Better water, worse food. Approximately 20 adults. \
Their leader, Petra Voss, is organised and suspicious of the town. Not hostile. Waiting.\n\
\n\
THE NIGHTS\n\
After dark, creatures move outside. No survivor has seen one clearly. What is known: \
they break bones, displace limbs, and make sounds that belong to nothing living. They cannot \
enter a properly sealed building — but the definition of 'properly sealed' has killed several \
people who were certain they understood it. They are drawn to light, movement, and sound. \
A creature can be driven off, rarely killed. Killing one does not make the others retreat.\n\
\n\
THE VOICES\n\
Some residents hear voices. Specific instructions. Names of people. Descriptions of places. \
The instructions are not always wrong. Following them too completely ends badly. The voices \
know more than is possible and less than would be useful. Their source is unknown.\n\
\n\
RESOURCES (Day 1 baseline)\n\
Food: community stores in the hall, rationed daily. ~20 days at current consumption. \
Colony house has ~8 days independently.\n\
Medicine: the clinic has antibiotics, bandages, painkillers, two doses of morphine. No IV. \
No surgical capability.\n\
Fuel: diesel generator at the colony house powers one building. ~10 nights of fuel remain. \
The town relies on candles, oil lamps, and a fireplace in the hall.\n\
Weapons: three hunting rifles in the town (one with Lloyd Becker, one in the hardware store, \
one unaccounted for since an early incident), one shotgun at the colony house with Petra Voss, \
hunting knives throughout.\n\
\n\
KEY FIGURES\n\
\n\
Town:\n\
Lloyd Becker — former mechanic, de facto town leader. Pragmatic to the point of cruelty \
when necessary. Father of two; his wife was taken on Night 4 before the player arrived.\n\
Father Donal Creighton — priest at St. Brendan's. Provides comfort; hides growing doubt \
under ritual. The church is the safest-feeling building in town, which is not the same as safe.\n\
Nadia Osei — former nurse, runs the clinic. Most respected person in town. Stretched thin. \
Knows more about who is deteriorating than she says.\n\
\n\
Colony House:\n\
Petra Voss — organised, calm, former schoolteacher. Made hard decisions in the early days \
that no one discusses. Distrusts the town but will negotiate.\n\
Marcus Voss — Petra's husband. Quieter, more frightened. Sometimes heard talking to himself.\n\
\n\
Voices followers (fluid):\n\
Old Ren Whitaker — has been here longer than anyone else. Appears unconcerned about dying. \
The voices do not seem to trouble him — he seems to be listening for something else entirely.\n\
Hazel Fenn — former graduate student. Has followed two voice instructions that proved correct. \
Becoming a true believer. This will end badly.\n\
Emil Dracho — middle-aged, quiet. May or may not hear voices. Behaves as if he does.\n\
\n\
The Mathers family — Cora and her adult children Bram and Theo — have partially withdrawn \
from community life. Cora believes the valley has a logic that can be learned. She is not \
entirely wrong about there being a logic. She is mostly wrong about which part she has learned.\n\
\n\
The Stranger — a figure seen occasionally at the forest edge. Has not spoken. Has not attacked. \
Three separate witnesses gave three inconsistent descriptions. The town has decided collectively \
not to discuss this.\n\
\n\
THE 17-DAY ARC\n\
Days 1–4: Orientation. Shock has passed. People are frightened but functional. Early \
casualties are possible; they are not expected.\n\
Days 5–8: First fractures. Rationing creates tension. The colony house relationship \
deteriorates or stabilises. First voices-arc event probable.\n\
Days 9–12: Mid-game crisis. At least one major faction event — betrayal, expulsion, or \
open conflict. Voices followers begin making visible moves.\n\
Days 13–16: Collapse. Multiple deaths expected. The structures built in the first week \
are tested to destruction.\n\
Day 17: Resolution. The pattern either breaks or it does not. The win condition involves \
two specific residents reaching a specific state — the Director must never name or hint at \
who they are; it must emerge through play. Most playthroughs do not reach the win condition.\n\
\n\
TONE\n\
Every choice has consequences. Kindness costs something. Safety is temporary. People divide \
not into good and evil but into frightened and very frightened, and what each does with that \
fear is what the story is made of. Small human moments — a shared meal, a lie told to protect \
someone, a hand held in the dark — matter as much as the creature attacks. Write slow dread. \
Earn the horror.\
";
