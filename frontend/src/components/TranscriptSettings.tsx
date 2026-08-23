import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './ui/select';
import { Input } from './ui/input';
import { Button } from './ui/button';
import { Label } from './ui/label';
import { Eye, EyeOff, Globe, Lock, Unlock } from 'lucide-react';
import { ModelManager } from './WhisperModelManager';
import { ParakeetModelManager } from './ParakeetModelManager';

interface RemoteTranscriptionConfig {
  endpoint: string;
  model: string;
  api_key?: string | null;
}


export interface TranscriptModelProps {
    provider: 'localWhisper' | 'parakeet' | 'deepgram' | 'elevenLabs' | 'groq' | 'openai';
    model: string;
    apiKey?: string | null;
}

export interface TranscriptSettingsProps {
    transcriptModelConfig: TranscriptModelProps;
    setTranscriptModelConfig: (config: TranscriptModelProps) => void;
    onModelSelect?: () => void;
}

export function TranscriptSettings({ transcriptModelConfig, setTranscriptModelConfig, onModelSelect }: TranscriptSettingsProps) {
    const [apiKey, setApiKey] = useState<string | null>(transcriptModelConfig.apiKey || null);
    const [showApiKey, setShowApiKey] = useState<boolean>(false);
    const [isApiKeyLocked, setIsApiKeyLocked] = useState<boolean>(true);
    const [isLockButtonVibrating, setIsLockButtonVibrating] = useState<boolean>(false);
    const [uiProvider, setUiProvider] = useState<TranscriptModelProps['provider']>(transcriptModelConfig.provider);

    // Remote endpoint for batch retranscription. Deliberately NOT one of the
    // provider choices above: live recording only accepts local engines, and
    // putting the endpoint in that select would suggest otherwise. A saved
    // endpoint shows up as an extra model option in the retranscription dialog.
    const [remoteEndpoint, setRemoteEndpoint] = useState<string>('');
    const [remoteModel, setRemoteModel] = useState<string>('');
    const [remoteApiKey, setRemoteApiKey] = useState<string>('');
    const [showRemoteApiKey, setShowRemoteApiKey] = useState<boolean>(false);
    const [remoteConfigured, setRemoteConfigured] = useState<boolean>(false);
    const [remoteStatus, setRemoteStatus] = useState<string | null>(null);

    useEffect(() => {
        invoke<RemoteTranscriptionConfig | null>('api_get_remote_transcription_config')
            .then((config) => {
                if (config) {
                    setRemoteEndpoint(config.endpoint);
                    setRemoteModel(config.model);
                    setRemoteApiKey(config.api_key || '');
                    setRemoteConfigured(true);
                }
            })
            .catch((err) => console.error('Failed to load remote transcription config:', err));
    }, []);

    const handleSaveRemoteConfig = async () => {
        setRemoteStatus(null);
        try {
            await invoke('api_save_remote_transcription_config', {
                endpoint: remoteEndpoint.trim(),
                model: remoteModel.trim(),
                apiKey: remoteApiKey.trim() || null,
            });
            setRemoteConfigured(true);
            setRemoteStatus('Saved. The endpoint is now available in the retranscription dialog.');
        } catch (err) {
            setRemoteStatus(typeof err === 'string' ? err : 'Failed to save remote endpoint');
        }
    };

    const handleClearRemoteConfig = async () => {
        setRemoteStatus(null);
        try {
            await invoke('api_clear_remote_transcription_config');
            setRemoteEndpoint('');
            setRemoteModel('');
            setRemoteApiKey('');
            setRemoteConfigured(false);
            setRemoteStatus('Removed.');
        } catch (err) {
            setRemoteStatus(typeof err === 'string' ? err : 'Failed to remove remote endpoint');
        }
    };

    // Sync uiProvider when backend config changes (e.g., after model selection or initial load)
    useEffect(() => {
        setUiProvider(transcriptModelConfig.provider);
    }, [transcriptModelConfig.provider]);

    useEffect(() => {
        if (transcriptModelConfig.provider === 'localWhisper' || transcriptModelConfig.provider === 'parakeet') {
            setApiKey(null);
        }
    }, [transcriptModelConfig.provider]);

    const fetchApiKey = async (provider: string) => {
        try {

            const data = await invoke('api_get_transcript_api_key', { provider }) as string;

            setApiKey(data || '');
        } catch (err) {
            console.error('Error fetching API key:', err);
            setApiKey(null);
        }
    };
    const modelOptions = {
        localWhisper: [], // Model selection handled by ModelManager component
        parakeet: [], // Model selection handled by ParakeetModelManager component
        deepgram: ['nova-2-phonecall'],
        elevenLabs: ['eleven_multilingual_v2'],
        groq: ['llama-3.3-70b-versatile'],
        openai: ['gpt-4o'],
    };
    const requiresApiKey = transcriptModelConfig.provider === 'deepgram' || transcriptModelConfig.provider === 'elevenLabs' || transcriptModelConfig.provider === 'openai' || transcriptModelConfig.provider === 'groq';

    const handleInputClick = () => {
        if (isApiKeyLocked) {
            setIsLockButtonVibrating(true);
            setTimeout(() => setIsLockButtonVibrating(false), 500);
        }
    };

    const handleWhisperModelSelect = (modelName: string) => {
        // Always update config when model is selected, regardless of current provider
        // This ensures the model is set when user switches back
        setTranscriptModelConfig({
            ...transcriptModelConfig,
            provider: 'localWhisper', // Ensure provider is set correctly
            model: modelName
        });
        // Close modal after selection
        if (onModelSelect) {
            onModelSelect();
        }
    };

    const handleParakeetModelSelect = (modelName: string) => {
        // Always update config when model is selected, regardless of current provider
        // This ensures the model is set when user switches back
        setTranscriptModelConfig({
            ...transcriptModelConfig,
            provider: 'parakeet', // Ensure provider is set correctly
            model: modelName
        });
        // Close modal after selection
        if (onModelSelect) {
            onModelSelect();
        }
    };

    return (
        <div>
            <div>
                {/* <div className="flex justify-between items-center mb-4">
                    <h3 className="text-lg font-semibold text-foreground">Transcript Settings</h3>
                </div> */}
                <div className="space-y-4 pb-6">
                    <div>
                        <Label className="block text-sm font-medium text-foreground mb-1">
                            Transcript Model
                        </Label>
                        <div className="flex space-x-2 mx-1">
                            <Select
                                value={uiProvider}
                                onValueChange={(value) => {
                                    const provider = value as TranscriptModelProps['provider'];
                                    setUiProvider(provider);
                                    if (provider !== 'localWhisper' && provider !== 'parakeet') {
                                        fetchApiKey(provider);
                                    }
                                }}
                            >
                                <SelectTrigger className='focus:ring-1 focus:ring-blue-500 focus:border-blue-500'>
                                    <SelectValue placeholder="Select provider" />
                                </SelectTrigger>
                                <SelectContent>
                                    <SelectItem value="parakeet">⚡ Parakeet (Recommended - Real-time / Accurate)</SelectItem>
                                    <SelectItem value="localWhisper">🏠 Local Whisper (High Accuracy)</SelectItem>
                                    {/* <SelectItem value="deepgram">☁️ Deepgram (Backup)</SelectItem>
                                    <SelectItem value="elevenLabs">☁️ ElevenLabs</SelectItem>
                                    <SelectItem value="groq">☁️ Groq</SelectItem>
                                    <SelectItem value="openai">☁️ OpenAI</SelectItem> */}
                                </SelectContent>
                            </Select>

                            {uiProvider !== 'localWhisper' && uiProvider !== 'parakeet' && (
                                <Select
                                    value={transcriptModelConfig.model}
                                    onValueChange={(value) => {
                                        const model = value as TranscriptModelProps['model'];
                                        setTranscriptModelConfig({ ...transcriptModelConfig, provider: uiProvider, model });
                                    }}
                                >
                                    <SelectTrigger className='focus:ring-1 focus:ring-blue-500 focus:border-blue-500'>
                                        <SelectValue placeholder="Select model" />
                                    </SelectTrigger>
                                    <SelectContent>
                                        {modelOptions[uiProvider].map((model) => (
                                            <SelectItem key={model} value={model}>{model}</SelectItem>
                                        ))}
                                    </SelectContent>
                                </Select>
                            )}

                        </div>
                    </div>

                    {uiProvider === 'localWhisper' && (
                        <div className="mt-6">
                            <ModelManager
                                selectedModel={transcriptModelConfig.provider === 'localWhisper' ? transcriptModelConfig.model : undefined}
                                onModelSelect={handleWhisperModelSelect}
                                autoSave={true}
                            />
                        </div>
                    )}

                    {uiProvider === 'parakeet' && (
                        <div className="mt-6">
                            <ParakeetModelManager
                                selectedModel={transcriptModelConfig.provider === 'parakeet' ? transcriptModelConfig.model : undefined}
                                onModelSelect={handleParakeetModelSelect}
                                autoSave={true}
                            />
                        </div>
                    )}

                    <div className="mt-8 pt-6 border-t border-border">
                        <div className="flex items-center gap-2 mb-1">
                            <Globe className="h-4 w-4 text-muted-foreground" />
                            <Label className="text-sm font-medium text-foreground">
                                Remote transcription endpoint
                            </Label>
                        </div>
                        <p className="text-xs text-muted-foreground mb-3">
                            An OpenAI-compatible server (OpenAI, Groq, a self-hosted whisper.cpp
                            server, ...) used only when re-transcribing existing meetings — live
                            recording always stays local. The API key is optional; self-hosted
                            servers usually don't need one.
                        </p>
                        <div className="space-y-2 mx-1">
                            <Input
                                type="text"
                                placeholder="Endpoint URL, e.g. http://127.0.0.1:8080/v1"
                                value={remoteEndpoint}
                                onChange={(e) => setRemoteEndpoint(e.target.value)}
                                className="focus:ring-1 focus:ring-blue-500 focus:border-blue-500"
                            />
                            <Input
                                type="text"
                                placeholder="Model name, e.g. whisper-large-v3"
                                value={remoteModel}
                                onChange={(e) => setRemoteModel(e.target.value)}
                                className="focus:ring-1 focus:ring-blue-500 focus:border-blue-500"
                            />
                            <div className="relative">
                                <Input
                                    type={showRemoteApiKey ? 'text' : 'password'}
                                    placeholder="API key (optional)"
                                    value={remoteApiKey}
                                    onChange={(e) => setRemoteApiKey(e.target.value)}
                                    className="pr-12 focus:ring-1 focus:ring-blue-500 focus:border-blue-500"
                                />
                                <div className="absolute inset-y-0 right-0 pr-1 flex items-center">
                                    <Button
                                        type="button"
                                        variant="ghost"
                                        size="icon"
                                        onClick={() => setShowRemoteApiKey(!showRemoteApiKey)}
                                    >
                                        {showRemoteApiKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                                    </Button>
                                </div>
                            </div>
                            <div className="flex gap-2 pt-1">
                                <Button
                                    type="button"
                                    onClick={handleSaveRemoteConfig}
                                    disabled={!remoteEndpoint.trim() || !remoteModel.trim()}
                                >
                                    Save endpoint
                                </Button>
                                {remoteConfigured && (
                                    <Button type="button" variant="outline" onClick={handleClearRemoteConfig}>
                                        Remove
                                    </Button>
                                )}
                            </div>
                            {remoteStatus && (
                                <p className="text-xs text-muted-foreground">{remoteStatus}</p>
                            )}
                        </div>
                    </div>


                    {requiresApiKey && (
                        <div>
                            <Label className="block text-sm font-medium text-foreground mb-1">
                                API Key
                            </Label>
                            <div className="relative mx-1">
                                <Input
                                    type={showApiKey ? "text" : "password"}
                                    className={`pr-24 focus:ring-1 focus:ring-blue-500 focus:border-blue-500 ${isApiKeyLocked ? 'bg-muted cursor-not-allowed' : ''
                                        }`}
                                    value={apiKey || ''}
                                    onChange={(e) => setApiKey(e.target.value)}
                                    disabled={isApiKeyLocked}
                                    onClick={handleInputClick}
                                    placeholder="Enter your API key"
                                />
                                {isApiKeyLocked && (
                                    <div
                                        onClick={handleInputClick}
                                        className="absolute inset-0 flex items-center justify-center bg-muted/50 rounded-md cursor-not-allowed"
                                    />
                                )}
                                <div className="absolute inset-y-0 right-0 pr-1 flex items-center">
                                    <Button
                                        type="button"
                                        variant="ghost"
                                        size="icon"
                                        onClick={() => setIsApiKeyLocked(!isApiKeyLocked)}
                                        className={`transition-colors duration-200 ${isLockButtonVibrating ? 'animate-vibrate text-red-500' : ''
                                            }`}
                                        title={isApiKeyLocked ? "Unlock to edit" : "Lock to prevent editing"}
                                    >
                                        {isApiKeyLocked ? <Lock className="h-4 w-4" /> : <Unlock className="h-4 w-4" />}
                                    </Button>
                                    <Button
                                        type="button"
                                        variant="ghost"
                                        size="icon"
                                        onClick={() => setShowApiKey(!showApiKey)}
                                    >
                                        {showApiKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                                    </Button>
                                </div>
                            </div>
                        </div>
                    )}
                </div>
            </div>
        </div >
    )
}








