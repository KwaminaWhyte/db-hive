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
    <div style={{ display: "flex", height: "100vh" }}>
      {/* Left Side - Connection List */}
      <div
        style={{
          width: "40%",
          borderRight: "1px solid #ccc",
          overflowY: "auto",
        }}
      >
        <ConnectionList
          key={refreshKey}
          onEdit={handleEdit}
          onProfilesChange={() => setRefreshKey((prev) => prev + 1)}
        />
      </div>

      {/* Right Side - Connection Form */}
      <div
        style={{
          width: "60%",
          overflowY: "auto",
        }}
      >
        <ConnectionForm
          profile={selectedProfile}
          onSuccess={handleProfileSaved}
        />
      </div>
    </div>
  );
}

export default App;
