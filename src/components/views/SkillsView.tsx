import { Play, Plus, Sparkles } from "lucide-react";
import { useState } from "react";
import { useGrokHub } from "@/lib/store";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";

export function SkillsView() {
  const skills = useGrokHub((s) => s.skills);
  const toggleSkill = useGrokHub((s) => s.toggleSkill);
  const runSkill = useGrokHub((s) => s.runSkill);
  const addSkill = useGrokHub((s) => s.addSkill);
  const running = useGrokHub((s) => s.running);
  const openClawWorkspace = useGrokHub((s) => s.openClawWorkspace);
  const setNav = useGrokHub((s) => s.setNav);

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
      {openClawWorkspace && (
        <Card>
          <CardContent className="flex flex-wrap items-center justify-between gap-2 py-3">
            <p className="text-xs text-[var(--color-muted)]">
              OpenClaw workspace linked · {openClawWorkspace.filesImported.length} files · manage in
              Settings
            </p>
            <Button size="sm" variant="secondary" onClick={() => setNav("settings")}>
              Settings
            </Button>
          </CardContent>
        </Card>
      )}

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
                    {sk.id.startsWith("ocskill") ? " · openclaw" : ""}
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
                      disabled={running || !sk.enabled}
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

        <Card className="h-fit">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-sm">
              <Plus className="h-4 w-4" />
              New skill
            </CardTitle>
            <CardDescription>Custom slash skill for this workspace.</CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Name"
            />
            <Input
              value={slash}
              onChange={(e) => setSlash(e.target.value)}
              placeholder="/slash"
              className="font-mono text-xs"
            />
            <Input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Short description"
            />
            <Textarea
              value={instructions}
              onChange={(e) => setInstructions(e.target.value)}
              placeholder="Instructions the agent should follow…"
              rows={5}
            />
            <Button className="w-full" onClick={onCreate} disabled={!name.trim() || !instructions.trim()}>
              Add skill
            </Button>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
