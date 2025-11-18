import { useState } from "react";
import "./App.css";
import { ConnectionForm } from "./components/ConnectionForm";
import { ConnectionList } from "./components/ConnectionList";
import { ConnectionProfile } from "./types/database";

function App() {
  const [selectedProfile, setSelectedProfile] = useState<
    ConnectionProfile | undefined
  >(undefined);
  const [refreshKey, setRefreshKey] = useState(0);

  // Handle successful profile save
  const handleProfileSaved = () => {
    // Trigger refresh of connection list
    setRefreshKey((prev) => prev + 1);
    // Clear selected profile
    setSelectedProfile(undefined);
  };

  // Handle edit button click
  const handleEdit = (profile: ConnectionProfile) => {
    setSelectedProfile(profile);
  };

  return (
    <div className="flex h-screen w-full bg-background">
      {/* Left Side - Connection List */}
      <div className="w-2/5 border-r overflow-y-auto">
        <ConnectionList
          key={refreshKey}
          onEdit={handleEdit}
          onProfilesChange={() => setRefreshKey((prev) => prev + 1)}
        />
      </div>

      {/* Right Side - Connection Form */}
      <div className="w-3/5 overflow-y-auto">
        <ConnectionForm
          profile={selectedProfile}
          onSuccess={handleProfileSaved}
        />
      </div>
    </div>
  );
}

export default App;
