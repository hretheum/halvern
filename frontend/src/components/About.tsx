import React, { useState, useEffect } from "react";
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import AnalyticsConsentSwitch from "./AnalyticsConsentSwitch";
import { HalvernMark } from '@/components/brand/HalvernMark';


export function About() {
    const [currentVersion, setCurrentVersion] = useState<string>('0.4.0');

    useEffect(() => {
        // Get current version on mount
        getVersion().then(setCurrentVersion).catch(console.error);
    }, []);

    const handleUpstreamClick = async () => {
        try {
            await invoke('open_external_url', {
                url: 'https://github.com/Zackriya-Solutions/meeting-minutes',
            });
        } catch (error) {
            console.error('Failed to open link:', error);
        }
    };

    return (
        <div className="p-4 space-y-4 h-[80vh] overflow-y-auto">
            {/* Compact Header */}
            <div className="text-center">
                <div className="mb-3">
                    <HalvernMark size={64} title="Halvern" className="mx-auto text-primary" />
                </div>
                {/* <h1 className="text-xl font-bold text-foreground">Halvern</h1> */}
                <span className="text-sm text-muted-foreground"> v{currentVersion}</span>
                <p className="text-medium text-muted-foreground mt-1">
                    Real-time notes and summaries that never leave your machine.
                </p>
            </div>

            {/* Features Grid - Compact */}
            <div className="space-y-3">
                <h2 className="text-base font-semibold text-foreground">What makes Halvern different</h2>
                <div className="grid grid-cols-2 gap-2">
                    <div className="bg-muted rounded p-3 hover:bg-accent transition-colors">
                        <h3 className="font-bold text-sm text-foreground mb-1">Privacy-first</h3>
                        <p className="text-xs text-muted-foreground leading-relaxed">Your data & AI processing workflow can now stay within your premise. No cloud, no leaks.</p>
                    </div>
                    <div className="bg-muted rounded p-3 hover:bg-accent transition-colors">
                        <h3 className="font-bold text-sm text-foreground mb-1">Use Any Model</h3>
                        <p className="text-xs text-muted-foreground leading-relaxed">Prefer local open-source model? Great. Want to plug in an external API? Also fine. No lock-in.</p>
                    </div>
                    <div className="bg-muted rounded p-3 hover:bg-accent transition-colors">
                        <h3 className="font-bold text-sm text-foreground mb-1">Cost-Smart</h3>
                        <p className="text-xs text-muted-foreground leading-relaxed">Avoid pay-per-minute bills by running models locally (or pay only for the calls you choose).</p>
                    </div>
                    <div className="bg-muted rounded p-3 hover:bg-accent transition-colors">
                        <h3 className="font-bold text-sm text-foreground mb-1">Works everywhere</h3>
                        <p className="text-xs text-muted-foreground leading-relaxed">Google Meet, Zoom, Teams-online or offline.</p>
                    </div>
                </div>
            </div>

            {/* Attribution. The licence requires the copyright notice to survive;
                keeping it visible in the product is the least a fork can do, and
                it is also the honest answer to "where did this come from". What
                is deliberately gone is upstream's sales funnel — a button
                inviting our users to hire their team, and a roadmap promise we
                do not own — which is a different thing from credit. */}
            <div className="pt-2 border-t border-border text-center space-y-1">
                <p className="text-xs text-muted-foreground">
                    Built on{' '}
                    <button
                        onClick={handleUpstreamClick}
                        className="underline underline-offset-2 hover:text-foreground transition-colors"
                    >
                        Meetily
                    </button>
                    {' '}by Zackriya Solutions, used under the MIT licence.
                </p>
                <p className="text-xs text-muted-foreground">
                    Halvern is an independent fork and is not affiliated with them.
                </p>
            </div>
            <AnalyticsConsentSwitch />
        </div>

    )
}