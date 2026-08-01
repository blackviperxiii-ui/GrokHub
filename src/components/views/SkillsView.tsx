import { Play, Plus, Sparkles } from "lucide-react";
import { useState } from "react";
import { useGrokClaw } from "@/lib/store";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";

export function SkillsView() {
  const skills = useGrokClaw((s) => s.skills);
  const toggleSkill = useGrokClaw((s) => s.toggleSkill);
  const runSkill = useGrokClaw((s) => s.runSkill);
  const addSkill = useGrokClaw((s) => s.addSkill);
  const running = useGrokClaw((s) => s.running);

  const [name, setName] = useState("");
  const [slash, setSlash] = useState("");
  const [description, setDescription] = useState("");
  const [instructions, setInstructions] = useState("");

  function onCreate() {
    if (!name.trim() || !instructions.trim()) return;
    addSkill({
      name: name.trim(),
      slash: slash.trim() || `/${name.trim().toLowerCase().replace(/\s+/g, "-")}`,
      description: description.trim() || "Custom skill",
      instructions: instructions.trim(),
    });
    setName("");
    setSlash("");
    setDescription("");
    setInstructions("");
  }

  return (
    <div className="space-y-5">
      <div className="grid gap-4 lg:grid-cols-[1fr_340px]">
        <div className="grid gap-3 sm:grid-cols-2">
          {skills.map((sk) => (
            <Card key={sk.id} className="flex flex-col">
              <CardHeader>
                <div className="flex items-start justify-between gap-2">
                  <div>
                    <CardTitle className="flex items-center gap-2 text-sm">
                      <Sparkles className="h-3.5 w-3.5 text-[var(--color-muted)]" />
                      {sk.name}
                    </CardTitle>
                    <CardDescription className="mt-1 font-mono text-xs">
                      {sk.slash}
                    </CardDescription>
                  </div>
                  <Badge variant={sk.kind === "builtin" ? "info" : "default"}>
                    {sk.kind}
                  </Badge>
                </div>
              </CardHeader>
              <CardContent className="mt-auto flex flex-1 flex-col gap-3">
                <p className="text-sm text-[var(--color-muted)]">{sk.description}</p>
                <p className="line-clamp-2 text-xs text-[var(--color-subtle)]">
                  {sk.instructions}
                </p>
                <div className="mt-auto flex items-center justify-between gap-2 pt-1">
                  <span className="tabular text-xs text-[var(--color-subtle)]">
                    {sk.runs} runs
                  </span>
                  <div className="flex gap-2">
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => toggleSkill(sk.id)}
                    >
                      {sk.enabled ? "Disable" : "Enable"}
                    </Button>
                    <Button
                      size="sm"
                      disabled={!sk.enabled || running}
                      onClick={() => void runSkill(sk.id)}
                    >
                      <Play className="h-3.5 w-3.5" />
                      Run
                    </Button>
                  </div>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>

        <Card className="h-fit lg:sticky lg:top-20">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-sm">
              <Plus className="h-4 w-4" />
              Skill Creator
            </CardTitle>
            <CardDescription>
              Teach once — slash command sticks across sessions (local demo memory).
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            <div className="space-y-1.5">
              <label className="text-xs font-medium text-[var(--color-muted)]">Name</label>
              <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Gig day planner" />
            </div>
            <div className="space-y-1.5">
              <label className="text-xs font-medium text-[var(--color-muted)]">Slash</label>
              <Input value={slash} onChange={(e) => setSlash(e.target.value)} placeholder="/gigs" />
            </div>
            <div className="space-y-1.5">
              <label className="text-xs font-medium text-[var(--color-muted)]">Description</label>
              <Input
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="What this skill does"
              />
            </div>
            <div className="space-y-1.5">
              <label className="text-xs font-medium text-[var(--color-muted)]">Instructions</label>
              <Textarea
                value={instructions}
                onChange={(e) => setInstructions(e.target.value)}
                placeholder="Steps the agent should always follow…"
              />
            </div>
            <Button className="w-full" onClick={onCreate} disabled={!name.trim() || !instructions.trim()}>
              Save skill
            </Button>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
